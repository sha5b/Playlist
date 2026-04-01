use std::io::Cursor;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use serde::{Deserialize, Serialize};

use super::queue::{PlayQueue, QueueTrack};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    Off,
    All,
    One,
}

pub enum PlayerCommand {
    Play {
        tracks: Vec<QueueTrack>,
        start_index: usize,
        /// Pre-created streaming source for instant playback (file read + header parse on caller's thread).
        /// When None, the audio thread creates the decoder (brief <2ms block).
        source: Option<(Decoder<Cursor<Vec<u8>>>, u64)>,
    },
    PlaySingle(QueueTrack),
    Pause,
    Resume,
    Stop,
    Next,
    Prev,
    Seek(f64),          // seconds
    SetVolume(f64),     // 0.0 - 1.0
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    AddToQueue(QueueTrack),
    AddNext(QueueTrack),
    MoveInQueue { from: usize, to: usize },
    RemoveFromQueue(usize), // order index
    ClearQueue,
    /// Graceful shutdown -- audio thread stops and exits
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackState {
    pub state: PlayerState,
    pub current_track: Option<QueueTrack>,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub queue_length: usize,
    pub queue_position: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum PlayerEvent {
    StateChanged(PlaybackState),
    TrackChanged(Option<QueueTrack>),
    Progress {
        position_ms: u64,
        duration_ms: u64,
    },
    QueueUpdated {
        tracks: Vec<QueueTrack>,
        position: Option<usize>,
    },
    Error(String),
}

pub struct AudioEngine {
    cmd_tx: Sender<PlayerCommand>,
    shared: Arc<RwLock<SharedState>>,
    thread_handle: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    ffmpeg_path: Option<String>,
}

struct SharedState {
    playback: PlaybackState,
    queue: PlayQueue,
}

/// Streaming decoder -- file bytes are in memory but samples are decoded on-demand.
/// Uses ~5MB per track (compressed file bytes) instead of ~100MB (raw f32 PCM).
type PreloadedSource = Decoder<Cursor<Vec<u8>>>;

impl AudioEngine {
    pub fn new(ffmpeg_path: Option<String>, event_callback: Box<dyn Fn(PlayerEvent) + Send + 'static>) -> Self {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();

        let shared = Arc::new(RwLock::new(SharedState {
            playback: PlaybackState {
                state: PlayerState::Stopped,
                current_track: None,
                position_ms: 0,
                duration_ms: 0,
                volume: 0.75,
                shuffle: false,
                repeat: RepeatMode::Off,
                queue_length: 0,
                queue_position: None,
            },
            queue: PlayQueue::new(),
        }));

        let shared_clone = Arc::clone(&shared);
        let ffmpeg_clone = ffmpeg_path.clone();

        let handle = std::thread::Builder::new()
            .name("audio-playback".into())
            .spawn(move || {
                // Elevate audio thread priority so downloads/ffmpeg can't starve it
                #[cfg(windows)]
                {
                    use windows_sys::Win32::System::Threading::*;
                    unsafe {
                        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
                    }
                }
                #[cfg(unix)]
                {
                    unsafe { libc::nice(-5); }
                }
                Self::audio_thread(cmd_rx, shared_clone, event_callback, ffmpeg_clone);
            })
            .expect("failed to spawn audio thread");

        Self {
            cmd_tx,
            shared,
            thread_handle: std::sync::Mutex::new(Some(handle)),
            ffmpeg_path,
        }
    }

    pub fn send(&self, cmd: PlayerCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(PlayerCommand::Shutdown);
        if let Ok(mut guard) = self.thread_handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn get_state(&self) -> PlaybackState {
        self.shared.read().unwrap().playback.clone()
    }

    pub fn get_queue(&self) -> (Vec<QueueTrack>, Option<usize>) {
        let shared = self.shared.read().unwrap();
        (shared.queue.get_ordered_tracks(), shared.queue.position())
    }

    pub fn ffmpeg_path(&self) -> Option<&str> {
        self.ffmpeg_path.as_deref()
    }

    /// Create a streaming decoder for a track — reads file into memory but does NOT
    /// decode samples upfront.  Decoding happens on-demand during playback.
    /// ~5MB memory per track (compressed) instead of ~100MB (raw PCM).
    /// Can be called from any thread (Tauri command handler, preload thread, etc).
    pub fn decode_track(file_path: &str, ffmpeg_path: Option<&str>) -> Result<(Decoder<Cursor<Vec<u8>>>, u64), String> {
        log::info!("[audio] Creating streaming decoder: {}", file_path);
        let data = std::fs::read(file_path)
            .map_err(|e| format!("Failed to read file '{}': {}", file_path, e))?;

        let file_size = data.len();
        let cursor = Cursor::new(data);
        match Decoder::new(cursor) {
            Ok(source) => {
                let duration_ms = source.total_duration()
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                log::debug!(
                    "[audio] Streaming decoder ready: file_size={}KB, duration={}ms",
                    file_size / 1024, duration_ms
                );

                Ok((source, duration_ms))
            }
            Err(original_err) => {
                // Fallback: transcode unsupported formats via ffmpeg
                if super::transcode::needs_transcode(file_path) {
                    if let Some(ffmpeg) = ffmpeg_path {
                        log::info!("[audio] Native decode failed, transcoding via ffmpeg: {}", file_path);
                        let wav_bytes = super::transcode::transcode_to_wav(file_path, ffmpeg)?;
                        let wav_size = wav_bytes.len();
                        let cursor = Cursor::new(wav_bytes);
                        let source = Decoder::new(cursor)
                            .map_err(|e| format!("Failed to decode transcoded WAV: {}", e))?;
                        let duration_ms = source.total_duration()
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);

                        log::debug!(
                            "[audio] Transcoded decoder ready: wav_size={}KB, duration={}ms",
                            wav_size / 1024, duration_ms
                        );

                        return Ok((source, duration_ms));
                    }
                }
                Err(format!("Failed to decode '{}': {}", file_path, original_err))
            }
        }
    }

    /// Create a fresh Sink connected to the audio output, with the given volume.
    /// In rodio 0.20, `Sink::stop()` permanently kills a sink (the `stopped` flag
    /// is never cleared), so we must create a new Sink each time we want to play
    /// something new.  New sinks start un-paused.
    fn make_sink(stream_handle: &OutputStreamHandle, volume: f32) -> Sink {
        let sink = Sink::try_new(stream_handle).expect("Failed to create audio sink");
        sink.set_volume(volume);
        sink
    }

    /// Try to preload the next track based on current queue state.
    fn try_preload_next(shared: &Arc<RwLock<SharedState>>, ffmpeg_path: Option<&str>) -> Option<(QueueTrack, PreloadedSource, u64)> {
        let s = shared.read().unwrap();
        let next_track = s.queue.peek_next(s.playback.repeat == RepeatMode::All)?;
        let track = next_track.clone();
        drop(s);
        match Self::decode_track(&track.file_path, ffmpeg_path) {
            Ok((source, dur)) => {
                log::debug!("[audio] Preloaded next track: {}", track.title);
                Some((track, source, dur))
            }
            Err(e) => {
                log::warn!("[audio] Failed to preload next track: {}", e);
                None
            }
        }
    }

    fn audio_thread(
        cmd_rx: Receiver<PlayerCommand>,
        shared: Arc<RwLock<SharedState>>,
        emit: Box<dyn Fn(PlayerEvent) + Send>,
        ffmpeg_path: Option<String>,
    ) {
        // Create audio output on this dedicated thread -- OutputStream must stay alive
        let (_stream, stream_handle) = match OutputStream::try_default() {
            Ok(s) => {
                log::info!("[audio] Audio output device initialized");
                s
            }
            Err(e) => {
                log::error!("[audio] Failed to open audio output: {}", e);
                emit(PlayerEvent::Error(format!("Failed to open audio output: {}", e)));
                // Keep thread alive to drain commands even without audio
                loop {
                    match cmd_rx.recv() {
                        Ok(PlayerCommand::Shutdown) | Err(_) => return,
                        _ => {}
                    }
                }
            }
        };

        let mut sink = Self::make_sink(&stream_handle, 0.75);
        sink.pause(); // Start paused — nothing to play yet

        let mut current_duration_ms: u64 = 0;
        let mut play_start: Option<Instant> = None;
        let mut accumulated_ms: u64 = 0;
        let mut last_progress_emit = Instant::now();
        let mut is_playing = false;
        let mut preloaded: Option<(QueueTrack, PreloadedSource, u64)> = None;

        log::info!("[audio] Audio thread ready, waiting for commands");

        loop {
            match cmd_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(cmd) => {
                    match cmd {
                        PlayerCommand::Play { tracks, start_index, source } => {
                            log::info!("[audio] Play: {} tracks, start_index={}", tracks.len(), start_index);
                            {
                                let mut s = shared.write().unwrap();
                                s.queue.set_tracks(tracks, start_index);
                            }
                            preloaded = None;
                            if let Some((src, dur)) = source {
                                // Streaming decoder created on caller's thread — instant append
                                let track = shared.read().unwrap().queue.current().cloned();
                                if let Some(track) = track {
                                    // Use decoder duration, fall back to DB duration if unavailable
                                    current_duration_ms = if dur > 0 { dur } else {
                                        track.duration_ms.unwrap_or(0) as u64
                                    };
                                    accumulated_ms = 0;
                                    sink.stop();
                                    sink = Self::make_sink(&stream_handle, sink.volume());
                                    sink.append(src);
                                    play_start = Some(Instant::now());
                                    is_playing = true;
                                    {
                                        let mut s = shared.write().unwrap();
                                        s.playback.state = PlayerState::Playing;
                                        s.playback.current_track = Some(track.clone());
                                        s.playback.position_ms = 0;
                                        s.playback.duration_ms = current_duration_ms;
                                        s.playback.queue_length = s.queue.len();
                                        s.playback.queue_position = s.queue.position();
                                        emit(PlayerEvent::TrackChanged(Some(track)));
                                        emit(PlayerEvent::StateChanged(s.playback.clone()));
                                        let q_tracks = s.queue.get_ordered_tracks();
                                        let q_pos = s.queue.position();
                                        emit(PlayerEvent::QueueUpdated { tracks: q_tracks, position: q_pos });
                                    }
                                }
                            } else {
                                Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref());
                            }
                        }
                        PlayerCommand::PlaySingle(track) => {
                            log::info!("[audio] PlaySingle: {}", track.title);
                            {
                                let mut s = shared.write().unwrap();
                                s.queue.set_tracks(vec![track], 0);
                            }
                            preloaded = None;
                            Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref());
                        }
                        PlayerCommand::Pause => {
                            log::info!("[audio] Pause");
                            sink.pause();
                            if let Some(start) = play_start.take() {
                                accumulated_ms += start.elapsed().as_millis() as u64;
                            }
                            is_playing = false;
                            {
                                let mut s = shared.write().unwrap();
                                s.playback.state = PlayerState::Paused;
                                s.playback.position_ms = accumulated_ms;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                        }
                        PlayerCommand::Resume => {
                            log::info!("[audio] Resume");
                            if !sink.empty() {
                                sink.play();
                                play_start = Some(Instant::now());
                                is_playing = true;
                                {
                                    let mut s = shared.write().unwrap();
                                    s.playback.state = PlayerState::Playing;
                                    emit(PlayerEvent::StateChanged(s.playback.clone()));
                                }
                            } else {
                                log::warn!("[audio] Resume called but sink is empty");
                            }
                        }
                        PlayerCommand::Stop => {
                            log::info!("[audio] Stop");
                            // Stop old sink explicitly — dropping only detaches (audio keeps playing)
                            sink.stop();
                            sink = Self::make_sink(&stream_handle, sink.volume());
                            sink.pause();
                            play_start = None;
                            accumulated_ms = 0;
                            current_duration_ms = 0;
                            is_playing = false;
                            preloaded = None;
                            {
                                let mut s = shared.write().unwrap();
                                s.playback.state = PlayerState::Stopped;
                                s.playback.current_track = None;
                                s.playback.position_ms = 0;
                                s.playback.duration_ms = 0;
                                s.queue.clear();
                                s.playback.queue_length = 0;
                                s.playback.queue_position = None;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                                emit(PlayerEvent::TrackChanged(None));
                            }
                        }
                        PlayerCommand::Next => {
                            log::info!("[audio] Next");
                            let next_track = {
                                let mut s = shared.write().unwrap();
                                let repeat = s.playback.repeat.clone();
                                if repeat == RepeatMode::One {
                                    s.queue.current().cloned()
                                } else {
                                    let t = s.queue.next().cloned();
                                    if t.is_none() && repeat == RepeatMode::All {
                                        s.queue.restart().cloned()
                                    } else {
                                        t
                                    }
                                }
                            };
                            if next_track.is_some() {
                                preloaded = None;
                                Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref());
                            } else {
                                log::info!("[audio] Queue ended (Next)");
                                sink.stop();
                                sink = Self::make_sink(&stream_handle, sink.volume());
                                sink.pause();
                                play_start = None;
                                accumulated_ms = 0;
                                is_playing = false;
                                preloaded = None;
                                {
                                    let mut s = shared.write().unwrap();
                                    s.playback.state = PlayerState::Stopped;
                                    s.playback.position_ms = 0;
                                    emit(PlayerEvent::StateChanged(s.playback.clone()));
                                }
                            }
                        }
                        PlayerCommand::Prev => {
                            log::info!("[audio] Prev");
                            let current_pos = accumulated_ms
                                + play_start.as_ref().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
                            if current_pos > 3000 {
                                // More than 3s in — restart current track
                                preloaded = None;
                                Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref());
                            } else {
                                let has_prev = shared.write().unwrap().queue.prev().is_some();
                                preloaded = None;
                                if has_prev {
                                    Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref());
                                } else {
                                    // At start of queue — restart current
                                    Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref());
                                }
                            }
                        }
                        PlayerCommand::Seek(seconds) => {
                            let pos_ms = (seconds * 1000.0) as u64;
                            log::info!("[audio] Seek to {}ms", pos_ms);
                            match sink.try_seek(Duration::from_secs_f64(seconds)) {
                                Ok(()) => {
                                    accumulated_ms = pos_ms;
                                    play_start = Some(Instant::now());
                                    is_playing = true;
                                    {
                                        let mut s = shared.write().unwrap();
                                        s.playback.position_ms = pos_ms;
                                        s.playback.state = PlayerState::Playing;
                                        emit(PlayerEvent::StateChanged(s.playback.clone()));
                                    }
                                }
                                Err(e) => {
                                    log::error!("[audio] Seek failed: {}", e);
                                    emit(PlayerEvent::Error(format!("Seek failed: {}", e)));
                                }
                            }
                        }
                        PlayerCommand::SetVolume(vol) => {
                            let vol = vol.clamp(0.0, 1.0);
                            sink.set_volume(vol as f32);
                            {
                                let mut s = shared.write().unwrap();
                                s.playback.volume = vol;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                        }
                        PlayerCommand::SetShuffle(shuffle) => {
                            log::info!("[audio] SetShuffle: {}", shuffle);
                            {
                                let mut s = shared.write().unwrap();
                                s.queue.set_shuffle(shuffle);
                                s.playback.shuffle = shuffle;
                                s.playback.queue_position = s.queue.position();
                                let tracks = s.queue.get_ordered_tracks();
                                let pos = s.queue.position();
                                emit(PlayerEvent::QueueUpdated { tracks, position: pos });
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                            preloaded = None;
                        }
                        PlayerCommand::SetRepeat(mode) => {
                            log::info!("[audio] SetRepeat: {:?}", mode);
                            {
                                let mut s = shared.write().unwrap();
                                s.playback.repeat = mode;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                            preloaded = None;
                        }
                        PlayerCommand::AddToQueue(track) => {
                            log::info!("[audio] AddToQueue: {}", track.title);
                            {
                                let mut s = shared.write().unwrap();
                                s.queue.add_track(track);
                                s.playback.queue_length = s.queue.len();
                                let tracks = s.queue.get_ordered_tracks();
                                let pos = s.queue.position();
                                emit(PlayerEvent::QueueUpdated { tracks, position: pos });
                            }
                            preloaded = None;
                        }
                        PlayerCommand::AddNext(track) => {
                            log::info!("[audio] AddNext: {}", track.title);
                            {
                                let mut s = shared.write().unwrap();
                                s.queue.add_next(track);
                                s.playback.queue_length = s.queue.len();
                                let tracks = s.queue.get_ordered_tracks();
                                let pos = s.queue.position();
                                emit(PlayerEvent::QueueUpdated { tracks, position: pos });
                            }
                            preloaded = None;
                        }
                        PlayerCommand::MoveInQueue { from, to } => {
                            log::info!("[audio] MoveInQueue: from={} to={}", from, to);
                            {
                                let mut s = shared.write().unwrap();
                                s.queue.move_in_queue(from, to);
                                s.playback.queue_position = s.queue.position();
                                let tracks = s.queue.get_ordered_tracks();
                                let pos = s.queue.position();
                                emit(PlayerEvent::QueueUpdated { tracks, position: pos });
                            }
                            preloaded = None;
                        }
                        PlayerCommand::RemoveFromQueue(order_idx) => {
                            log::info!("[audio] RemoveFromQueue: index={}", order_idx);
                            {
                                let mut s = shared.write().unwrap();
                                s.queue.remove_at_order_index(order_idx);
                                s.playback.queue_length = s.queue.len();
                                s.playback.queue_position = s.queue.position();
                                let tracks = s.queue.get_ordered_tracks();
                                let pos = s.queue.position();
                                emit(PlayerEvent::QueueUpdated { tracks, position: pos });
                            }
                            preloaded = None;
                        }
                        PlayerCommand::ClearQueue => {
                            log::info!("[audio] ClearQueue");
                            sink.stop();
                            sink = Self::make_sink(&stream_handle, sink.volume());
                            sink.pause();
                            play_start = None;
                            accumulated_ms = 0;
                            is_playing = false;
                            preloaded = None;
                            {
                                let mut s = shared.write().unwrap();
                                s.queue.clear();
                                s.playback.state = PlayerState::Stopped;
                                s.playback.current_track = None;
                                s.playback.position_ms = 0;
                                s.playback.duration_ms = 0;
                                s.playback.queue_length = 0;
                                s.playback.queue_position = None;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                                emit(PlayerEvent::TrackChanged(None));
                                emit(PlayerEvent::QueueUpdated { tracks: vec![], position: None });
                            }
                        }
                        PlayerCommand::Shutdown => {
                            log::info!("[audio] Audio thread shutting down gracefully");
                            return;
                        }
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // Fall through to progress/end-of-track check
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    log::info!("[audio] Command channel disconnected, exiting");
                    return;
                }
            }

            // Skip the entire progress/end-of-track block when not playing
            if !is_playing {
                continue;
            }

            // Try to preload next track when approaching end (~5s remaining)
            if preloaded.is_none() && current_duration_ms > 0 {
                let pos = accumulated_ms
                    + play_start.as_ref().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
                let remaining = current_duration_ms.saturating_sub(pos);
                if remaining < 5000 {
                    preloaded = Self::try_preload_next(&shared, ffmpeg_path.as_deref());
                }
            }

            // Update position for playing state
            let pos = accumulated_ms
                + play_start.as_ref().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);

            // Emit progress every ~500ms
            if last_progress_emit.elapsed() >= Duration::from_millis(450) {
                {
                    let mut s = shared.write().unwrap();
                    s.playback.position_ms = pos;
                }
                emit(PlayerEvent::Progress {
                    position_ms: pos,
                    duration_ms: current_duration_ms,
                });
                last_progress_emit = Instant::now();
            }

            // Check if track finished (sink is empty)
            if sink.empty() && current_duration_ms > 0 {
                log::info!("[audio] Track finished naturally");
                let (next_exists, use_preloaded) = {
                    let mut s = shared.write().unwrap();
                    let repeat = s.playback.repeat.clone();
                    if repeat == RepeatMode::One {
                        (s.queue.current().is_some(), false)
                    } else {
                        let t = s.queue.next().cloned();
                        if t.is_none() && repeat == RepeatMode::All {
                            (s.queue.restart().is_some(), false)
                        } else if t.is_some() {
                            let matches_preload = preloaded.as_ref().map_or(false, |(pt, _, _)| {
                                s.queue.current().map_or(false, |ct| ct.id == pt.id)
                            });
                            (true, matches_preload)
                        } else {
                            (false, false)
                        }
                    }
                };

                if next_exists && use_preloaded {
                    log::info!("[audio] Gapless transition (preloaded)");
                    let (track, source, dur) = preloaded.take().unwrap();
                    current_duration_ms = if dur > 0 { dur } else {
                        track.duration_ms.unwrap_or(0) as u64
                    };
                    accumulated_ms = 0;
                    // Stop old sink, then fresh sink for the new track
                    sink.stop();
                    sink = Self::make_sink(&stream_handle, sink.volume());
                    sink.append(source);
                    play_start = Some(Instant::now());
                    {
                        let mut s = shared.write().unwrap();
                        s.playback.state = PlayerState::Playing;
                        s.playback.current_track = Some(track.clone());
                        s.playback.position_ms = 0;
                        s.playback.duration_ms = current_duration_ms;
                        s.playback.queue_length = s.queue.len();
                        s.playback.queue_position = s.queue.position();
                        emit(PlayerEvent::TrackChanged(Some(track)));
                        emit(PlayerEvent::StateChanged(s.playback.clone()));
                    }
                } else if next_exists {
                    preloaded = None;
                    Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref());
                } else {
                    log::info!("[audio] Queue ended");
                    play_start = None;
                    accumulated_ms = 0;
                    current_duration_ms = 0;
                    is_playing = false;
                    preloaded = None;
                    // Stop old sink, fresh sink in paused state
                    sink.stop();
                    sink = Self::make_sink(&stream_handle, sink.volume());
                    sink.pause();
                    let mut s = shared.write().unwrap();
                    s.playback.state = PlayerState::Stopped;
                    s.playback.position_ms = 0;
                    emit(PlayerEvent::StateChanged(s.playback.clone()));
                }
            }
        }
    }

    fn play_current(
        sink: &mut Sink,
        stream_handle: &OutputStreamHandle,
        shared: &Arc<RwLock<SharedState>>,
        emit: &dyn Fn(PlayerEvent),
        current_duration_ms: &mut u64,
        play_start: &mut Option<Instant>,
        accumulated_ms: &mut u64,
        is_playing: &mut bool,
        ffmpeg_path: Option<&str>,
    ) {
        let track = shared.read().unwrap().queue.current().cloned();

        if let Some(track) = track {
            log::info!("[audio] play_current: \"{}\" ({})", track.title, track.file_path);

            match Self::decode_track(&track.file_path, ffmpeg_path) {
                Ok((source, dur)) => {
                    // Use decoder duration, fall back to DB duration if unavailable
                    *current_duration_ms = if dur > 0 { dur } else {
                        track.duration_ms.unwrap_or(0) as u64
                    };
                    *accumulated_ms = 0;

                    // Stop old sink explicitly — dropping only detaches (ghost audio).
                    // Then create a fresh Sink (rodio 0.20 stop() permanently kills a sink,
                    // so we must create a new one each time).
                    sink.stop();
                    *sink = Self::make_sink(stream_handle, sink.volume());
                    sink.append(source);
                    // New sinks start un-paused, so playback begins immediately
                    *play_start = Some(Instant::now());
                    *is_playing = true;

                    {
                        let mut s = shared.write().unwrap();
                        s.playback.state = PlayerState::Playing;
                        s.playback.current_track = Some(track.clone());
                        s.playback.position_ms = 0;
                        s.playback.duration_ms = *current_duration_ms;
                        s.playback.queue_length = s.queue.len();
                        s.playback.queue_position = s.queue.position();
                        emit(PlayerEvent::TrackChanged(Some(track)));
                        emit(PlayerEvent::StateChanged(s.playback.clone()));
                        let q_tracks = s.queue.get_ordered_tracks();
                        let q_pos = s.queue.position();
                        emit(PlayerEvent::QueueUpdated { tracks: q_tracks, position: q_pos });
                    }
                    log::info!("[audio] Playback started (duration={}ms)", dur);
                }
                Err(e) => {
                    log::error!("[audio] {}", e);
                    *is_playing = false;
                    emit(PlayerEvent::Error(e));
                }
            }
        } else {
            log::warn!("[audio] play_current: no current track in queue");
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}
