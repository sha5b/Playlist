use std::io::Cursor;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

/// How often the audio thread polls for commands (ms)
const CMD_POLL_INTERVAL_MS: u64 = 250;
/// How often to check if the default audio device changed (secs)
const DEVICE_CHECK_INTERVAL_SECS: u64 = 2;
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
    SkipTo(usize),          // order index — jump to specific position in queue
    ClearQueue,
    /// Switch audio output device. None = use OS default.
    SwitchDevice(Option<String>),
    /// Enable/disable ReplayGain-style volume normalization (applies from the
    /// next source that is appended — current playback is not re-amplified).
    SetNormalize(bool),
    /// Crossfade duration in ms (0 = off/gapless). Clamped to 0..=12000.
    SetCrossfade(u64),
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

/// Read the shared state, recovering gracefully if the lock was poisoned.
fn read_state(lock: &RwLock<SharedState>) -> std::sync::RwLockReadGuard<'_, SharedState> {
    lock.read().unwrap_or_else(|e| {
        log::error!("[audio] State lock poisoned (read), recovering");
        e.into_inner()
    })
}

/// Write the shared state, recovering gracefully if the lock was poisoned.
fn write_state(lock: &RwLock<SharedState>) -> std::sync::RwLockWriteGuard<'_, SharedState> {
    lock.write().unwrap_or_else(|e| {
        log::error!("[audio] State lock poisoned (write), recovering");
        e.into_inner()
    })
}

/// Streaming decoder -- file bytes are in memory but samples are decoded on-demand.
/// Uses ~5MB per track (compressed file bytes) instead of ~100MB (raw f32 PCM).
type PreloadedSource = Decoder<Cursor<Vec<u8>>>;

/// Maximum crossfade duration (ms).
const MAX_CROSSFADE_MS: u64 = 12_000;

/// An in-progress crossfade: the outgoing track keeps playing on its old sink
/// while the incoming track (already swapped in as the engine's main sink)
/// fades in. Fade progress is tracked as accumulated wall time so pausing
/// freezes the fade.
struct CrossfadeState {
    old_sink: Sink,
    duration_ms: u64,
    elapsed_ms: u64,
    last_tick: Instant,
}

/// Convert a dB gain to a linear amplitude factor.
fn gain_to_amp(gain_db: f64) -> f32 {
    10f32.powf(gain_db as f32 / 20.0)
}

/// Append a source to a sink, applying the track's normalization gain when
/// normalization is enabled and the track has been measured.
fn append_with_gain(sink: &Sink, source: PreloadedSource, gain_db: Option<f64>, normalize: bool) {
    match gain_db.filter(|_| normalize) {
        Some(g) if g.abs() > 0.01 => sink.append(source.amplify(gain_to_amp(g))),
        _ => sink.append(source),
    }
}

/// End a crossfade immediately: kill the outgoing sink and restore the
/// incoming (now main) sink to the full user volume. Safe to call when no
/// crossfade is active.
fn finish_crossfade(crossfade: &mut Option<CrossfadeState>, sink: &Sink, shared: &RwLock<SharedState>) {
    if let Some(cf) = crossfade.take() {
        cf.old_sink.stop();
        sink.set_volume(read_state(shared).playback.volume as f32);
    }
}

impl AudioEngine {
    pub fn new(
        ffmpeg_path: Option<String>,
        normalize: bool,
        crossfade_ms: u64,
        event_callback: Box<dyn Fn(PlayerEvent) + Send + 'static>,
    ) -> Self {
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
                        if SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) == 0 {
                            log::warn!("[audio] Failed to set Windows thread priority");
                        }
                    }
                }
                #[cfg(unix)]
                {
                    let ret = unsafe { libc::nice(-5) };
                    if ret == -1 {
                        log::warn!("[audio] Failed to set Unix thread priority (nice), may need elevated permissions");
                    }
                }
                Self::audio_thread(cmd_rx, shared_clone, event_callback, ffmpeg_clone, normalize, crossfade_ms);
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

    /// List available audio output devices. Returns (name, is_default) pairs.
    pub fn list_output_devices() -> Vec<(String, bool)> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let default_name = host.default_output_device().and_then(|d| d.name().ok());
        match host.output_devices() {
            Ok(devices) => devices
                .filter_map(|d| {
                    let name = d.name().ok()?;
                    let is_default = default_name.as_deref() == Some(&name);
                    Some((name, is_default))
                })
                .collect(),
            Err(_) => vec![],
        }
    }

    pub fn get_state(&self) -> PlaybackState {
        read_state(&self.shared).playback.clone()
    }

    pub fn get_queue(&self) -> (Vec<QueueTrack>, Option<usize>) {
        let shared = read_state(&self.shared);
        (shared.queue.get_ordered_tracks(), shared.queue.position())
    }

    pub fn ffmpeg_path(&self) -> Option<&str> {
        self.ffmpeg_path.as_deref()
    }

    /// Create a streaming decoder for a track — reads file into memory but does NOT
    /// decode samples upfront.  Decoding happens on-demand during playback.
    /// ~5MB memory per track (compressed) instead of ~100MB (raw PCM).
    /// Can be called from any thread (Tauri command handler, preload thread, etc).
    #[allow(clippy::type_complexity)]
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
    fn make_sink(stream_handle: &OutputStreamHandle, volume: f32) -> Result<Sink, String> {
        let sink = Sink::try_new(stream_handle)
            .map_err(|e| format!("Failed to create audio sink: {}. Audio device may have been disconnected.", e))?;
        sink.set_volume(volume);
        Ok(sink)
    }

    /// Try to preload the next track based on current queue state.
    fn try_preload_next(shared: &Arc<RwLock<SharedState>>, ffmpeg_path: Option<&str>) -> Option<(QueueTrack, PreloadedSource, u64)> {
        let s = read_state(shared);
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
        initial_normalize: bool,
        initial_crossfade_ms: u64,
    ) {
        // Create audio output on this dedicated thread -- OutputStream must stay alive
        let (mut _stream, mut stream_handle) = match OutputStream::try_default() {
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

        let mut sink = match Self::make_sink(&stream_handle, 0.75) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[audio] {}", e);
                emit(PlayerEvent::Error(e));
                // Keep thread alive to drain commands even without audio
                loop {
                    match cmd_rx.recv() {
                        Ok(PlayerCommand::Shutdown) | Err(_) => return,
                        _ => {}
                    }
                }
            }
        };
        sink.pause(); // Start paused — nothing to play yet

        let mut current_duration_ms: u64 = 0;
        let mut play_start: Option<Instant> = None;
        let mut accumulated_ms: u64 = 0;
        let mut last_progress_emit = Instant::now();
        let mut is_playing = false;
        let mut preloaded: Option<(QueueTrack, PreloadedSource, u64)> = None;
        let mut normalize = initial_normalize;
        let mut crossfade_ms = initial_crossfade_ms.min(MAX_CROSSFADE_MS);
        let mut crossfade: Option<CrossfadeState> = None;

        // Track the current default device so we can auto-switch when it changes
        // (e.g., user plugs in Bluetooth headphones)
        let mut current_device_name: Option<String> = {
            use cpal::traits::{DeviceTrait, HostTrait};
            cpal::default_host().default_output_device().and_then(|d| d.name().ok())
        };
        let mut explicit_device: Option<String> = None; // set when user manually picks a device
        let mut last_device_check = Instant::now();

        /// Try to create a sink; on failure emit an error event and skip to next command.
        macro_rules! make_sink_or_continue {
            ($handle:expr, $vol:expr, $emit:expr) => {
                match Self::make_sink($handle, $vol) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("[audio] {}", e);
                        $emit(PlayerEvent::Error(e));
                        continue;
                    }
                }
            };
        }

        log::info!("[audio] Audio thread ready, waiting for commands");

        loop {
            // Poll faster while a crossfade is active so the volume ramp is smooth.
            let poll_ms = if crossfade.is_some() { 50 } else { CMD_POLL_INTERVAL_MS };
            match cmd_rx.recv_timeout(Duration::from_millis(poll_ms)) {
                Ok(cmd) => {
                    match cmd {
                        PlayerCommand::Play { tracks, start_index, source } => {
                            log::info!("[audio] Play: {} tracks, start_index={}", tracks.len(), start_index);
                            finish_crossfade(&mut crossfade, &sink, &shared);
                            {
                                let mut s = write_state(&shared);
                                s.queue.set_tracks(tracks, start_index);
                            }
                            preloaded = None;
                            if let Some((src, dur)) = source {
                                // Streaming decoder created on caller's thread — instant append
                                let track = read_state(&shared).queue.current().cloned();
                                if let Some(track) = track {
                                    // Use decoder duration, fall back to DB duration if unavailable
                                    current_duration_ms = if dur > 0 { dur } else {
                                        track.duration_ms.unwrap_or(0) as u64
                                    };
                                    accumulated_ms = 0;
                                    sink.stop();
                                    sink = make_sink_or_continue!(&stream_handle, sink.volume(), emit);
                                    append_with_gain(&sink, src, track.gain_db, normalize);
                                    play_start = Some(Instant::now());
                                    is_playing = true;
                                    {
                                        let mut s = write_state(&shared);
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
                                } else {
                                    // Empty queue: stop any stale audio instead of
                                    // leaving the replaced queue playing underneath.
                                    sink.stop();
                                    is_playing = false;
                                    play_start = None;
                                    {
                                        let mut s = write_state(&shared);
                                        s.playback.state = PlayerState::Stopped;
                                        s.playback.current_track = None;
                                        s.playback.position_ms = 0;
                                        s.playback.duration_ms = 0;
                                        emit(PlayerEvent::StateChanged(s.playback.clone()));
                                    }
                                }
                            } else {
                                Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref(), normalize);
                            }
                        }
                        PlayerCommand::PlaySingle(track) => {
                            log::info!("[audio] PlaySingle: {}", track.title);
                            finish_crossfade(&mut crossfade, &sink, &shared);
                            {
                                let mut s = write_state(&shared);
                                s.queue.set_tracks(vec![track], 0);
                            }
                            preloaded = None;
                            Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref(), normalize);
                        }
                        PlayerCommand::Pause => {
                            log::info!("[audio] Pause");
                            sink.pause();
                            // Pause the outgoing side of an active crossfade too,
                            // freezing fade progress at its current point.
                            if let Some(cf) = crossfade.as_mut() {
                                cf.old_sink.pause();
                                cf.elapsed_ms += cf.last_tick.elapsed().as_millis() as u64;
                                cf.last_tick = Instant::now();
                            }
                            if let Some(start) = play_start.take() {
                                accumulated_ms += start.elapsed().as_millis() as u64;
                            }
                            is_playing = false;
                            {
                                let mut s = write_state(&shared);
                                s.playback.state = PlayerState::Paused;
                                s.playback.position_ms = accumulated_ms;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                        }
                        PlayerCommand::Resume => {
                            log::info!("[audio] Resume");
                            if !sink.empty() {
                                sink.play();
                                if let Some(cf) = crossfade.as_mut() {
                                    cf.old_sink.play();
                                    cf.last_tick = Instant::now();
                                }
                                play_start = Some(Instant::now());
                                is_playing = true;
                                {
                                    let mut s = write_state(&shared);
                                    s.playback.state = PlayerState::Playing;
                                    emit(PlayerEvent::StateChanged(s.playback.clone()));
                                }
                            } else {
                                log::warn!("[audio] Resume called but sink is empty");
                            }
                        }
                        PlayerCommand::Stop => {
                            log::info!("[audio] Stop");
                            finish_crossfade(&mut crossfade, &sink, &shared);
                            // Stop old sink explicitly — dropping only detaches (audio keeps playing)
                            sink.stop();
                            sink = make_sink_or_continue!(&stream_handle, sink.volume(), emit);
                            sink.pause();
                            play_start = None;
                            accumulated_ms = 0;
                            current_duration_ms = 0;
                            is_playing = false;
                            preloaded = None;
                            {
                                let mut s = write_state(&shared);
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
                            finish_crossfade(&mut crossfade, &sink, &shared);
                            let next_track = {
                                let mut s = write_state(&shared);
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
                                Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref(), normalize);
                            } else {
                                log::info!("[audio] Queue ended (Next)");
                                sink.stop();
                                sink = make_sink_or_continue!(&stream_handle, sink.volume(), emit);
                                sink.pause();
                                play_start = None;
                                accumulated_ms = 0;
                                is_playing = false;
                                preloaded = None;
                                {
                                    let mut s = write_state(&shared);
                                    s.playback.state = PlayerState::Stopped;
                                    s.playback.position_ms = 0;
                                    emit(PlayerEvent::StateChanged(s.playback.clone()));
                                }
                            }
                        }
                        PlayerCommand::Prev => {
                            log::info!("[audio] Prev");
                            finish_crossfade(&mut crossfade, &sink, &shared);
                            // Always go to previous track. If there is no previous
                            // track in the queue, restart the current one.
                            let has_prev = write_state(&shared).queue.prev().is_some();
                            preloaded = None;
                            Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref(), normalize);
                            if !has_prev {
                                log::info!("[audio] No previous track, restarted current");
                            }
                        }
                        PlayerCommand::Seek(seconds) => {
                            let pos_ms = (seconds * 1000.0) as u64;
                            log::info!("[audio] Seek to {}ms", pos_ms);
                            // Seeking during an overlap ends the crossfade immediately
                            // (the seek target refers to the incoming/current track).
                            finish_crossfade(&mut crossfade, &sink, &shared);
                            // Seeking an EMPTY sink returns Ok but does nothing —
                            // don't pretend we're playing something.
                            if sink.empty() {
                                log::info!("[audio] Seek ignored: nothing loaded");
                                continue;
                            }
                            match sink.try_seek(Duration::from_secs_f64(seconds)) {
                                Ok(()) => {
                                    // Preserve the play/pause state: rodio's
                                    // try_seek does NOT unpause a paused sink,
                                    // so flipping to "playing" here made the UI
                                    // count up while audio stayed silent.
                                    accumulated_ms = pos_ms;
                                    if is_playing {
                                        play_start = Some(Instant::now());
                                    }
                                    {
                                        let mut s = write_state(&shared);
                                        s.playback.position_ms = pos_ms;
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
                            // During a crossfade the sink volumes are ramped by the
                            // fade tick (which reads the shared volume), so don't
                            // clobber the ramp here.
                            if crossfade.is_none() {
                                sink.set_volume(vol as f32);
                            }
                            {
                                let mut s = write_state(&shared);
                                s.playback.volume = vol;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                        }
                        PlayerCommand::SetShuffle(shuffle) => {
                            log::info!("[audio] SetShuffle: {}", shuffle);
                            {
                                let mut s = write_state(&shared);
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
                                let mut s = write_state(&shared);
                                s.playback.repeat = mode;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                            preloaded = None;
                        }
                        PlayerCommand::AddToQueue(track) => {
                            log::info!("[audio] AddToQueue: {}", track.title);
                            {
                                let mut s = write_state(&shared);
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
                                let mut s = write_state(&shared);
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
                                let mut s = write_state(&shared);
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
                                let mut s = write_state(&shared);
                                s.queue.remove_at_order_index(order_idx);
                                s.playback.queue_length = s.queue.len();
                                s.playback.queue_position = s.queue.position();
                                let tracks = s.queue.get_ordered_tracks();
                                let pos = s.queue.position();
                                emit(PlayerEvent::QueueUpdated { tracks, position: pos });
                            }
                            preloaded = None;
                        }
                        PlayerCommand::SkipTo(order_idx) => {
                            log::info!("[audio] SkipTo: index={}", order_idx);
                            let found = {
                                let mut s = write_state(&shared);
                                s.queue.skip_to(order_idx).is_some()
                            };
                            if found {
                                finish_crossfade(&mut crossfade, &sink, &shared);
                                preloaded = None;
                                Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref(), normalize);
                            }
                        }
                        PlayerCommand::ClearQueue => {
                            log::info!("[audio] ClearQueue");
                            finish_crossfade(&mut crossfade, &sink, &shared);
                            sink.stop();
                            sink = make_sink_or_continue!(&stream_handle, sink.volume(), emit);
                            sink.pause();
                            play_start = None;
                            accumulated_ms = 0;
                            is_playing = false;
                            preloaded = None;
                            {
                                let mut s = write_state(&shared);
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
                        PlayerCommand::SwitchDevice(device_name) => {
                            log::info!("[audio] SwitchDevice: {:?}", device_name);
                            finish_crossfade(&mut crossfade, &sink, &shared);
                            // Only record the explicit device AFTER a successful
                            // switch — recording it up front permanently disabled
                            // default-device auto-switching on failure.
                            let result = if let Some(ref name) = device_name {
                                use cpal::traits::{DeviceTrait, HostTrait};
                                let host = cpal::default_host();
                                match host.output_devices() {
                                    Ok(mut devices) => {
                                        match devices.find(|d| d.name().ok().as_deref() == Some(name.as_str())) {
                                            Some(device) => OutputStream::try_from_device(&device),
                                            None => Err(rodio::StreamError::NoDevice),
                                        }
                                    }
                                    Err(_) => Err(rodio::StreamError::NoDevice),
                                }
                            } else {
                                OutputStream::try_default()
                            };

                            match result {
                                Ok((new_stream, new_handle)) => {
                                    explicit_device = device_name.clone();
                                    // Resume playback on the new device instead of
                                    // killing it (the old code stopped playback and
                                    // left a track loaded that Resume couldn't restart).
                                    let vol = sink.volume();
                                    let was_playing = is_playing;
                                    let resume_pos = accumulated_ms
                                        + play_start.as_ref().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);

                                    sink.stop();
                                    _stream = new_stream;
                                    stream_handle = new_handle;
                                    sink = make_sink_or_continue!(&stream_handle, vol, emit);
                                    preloaded = None;
                                    current_device_name = device_name.clone().or_else(|| {
                                        use cpal::traits::{DeviceTrait, HostTrait};
                                        cpal::default_host().default_output_device().and_then(|d| d.name().ok())
                                    });

                                    let current_track = read_state(&shared).playback.current_track.clone();
                                    if let Some(ref track) = current_track {
                                        match Self::decode_track(&track.file_path, ffmpeg_path.as_deref()) {
                                            Ok((source, dur)) => {
                                                if dur > 0 { current_duration_ms = dur; }
                                                append_with_gain(&sink, source, track.gain_db, normalize);
                                                if resume_pos > 0 {
                                                    sink.try_seek(Duration::from_millis(resume_pos))
                                                        .unwrap_or_else(|e| log::warn!("[audio] Seek after device switch failed: {}", e));
                                                }
                                                accumulated_ms = resume_pos;
                                                if was_playing {
                                                    play_start = Some(Instant::now());
                                                    is_playing = true;
                                                } else {
                                                    sink.pause();
                                                    play_start = None;
                                                    is_playing = false;
                                                }
                                            }
                                            Err(e) => {
                                                log::warn!("[audio] Failed to re-decode after device switch: {}", e);
                                                sink.pause();
                                                play_start = None;
                                                accumulated_ms = 0;
                                                is_playing = false;
                                                let mut s = write_state(&shared);
                                                s.playback.state = PlayerState::Stopped;
                                                s.playback.position_ms = 0;
                                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                                            }
                                        }
                                    } else {
                                        sink.pause();
                                        play_start = None;
                                        accumulated_ms = 0;
                                        is_playing = false;
                                    }
                                    log::info!("[audio] Switched to device: {:?} (resumed at {}ms, playing={})", device_name, resume_pos, is_playing);
                                }
                                Err(e) => {
                                    log::error!("[audio] Failed to switch device: {}", e);
                                    emit(PlayerEvent::Error(format!("Failed to switch audio device: {}", e)));
                                }
                            }
                        }
                        PlayerCommand::SetNormalize(enabled) => {
                            log::info!("[audio] SetNormalize: {}", enabled);
                            normalize = enabled;
                            // Applies from the next appended source; current playback
                            // keeps its amplification until the track changes.
                            preloaded = None;
                        }
                        PlayerCommand::SetCrossfade(ms) => {
                            let ms = ms.min(MAX_CROSSFADE_MS);
                            log::info!("[audio] SetCrossfade: {}ms", ms);
                            crossfade_ms = ms;
                            if ms == 0 {
                                finish_crossfade(&mut crossfade, &sink, &shared);
                            }
                        }
                        PlayerCommand::Shutdown => {
                            log::info!("[audio] Audio thread shutting down gracefully");
                            if let Some(cf) = crossfade.take() {
                                cf.old_sink.stop();
                            }
                            return;
                        }
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // Periodically check if the OS default audio device changed
                    // (e.g., Bluetooth headphones connected/disconnected).
                    // Only auto-switch if the user hasn't manually selected a device.
                    let should_check_device = explicit_device.is_none()
                        && last_device_check.elapsed() >= Duration::from_secs(DEVICE_CHECK_INTERVAL_SECS);
                    if !should_check_device {
                        // Nothing to do this tick
                    } else {
                        last_device_check = Instant::now();
                        let new_default: Option<String> = {
                            use cpal::traits::{DeviceTrait, HostTrait};
                            cpal::default_host().default_output_device().and_then(|d| d.name().ok())
                        };
                        if new_default == current_device_name {
                            // Device unchanged, nothing to do
                        } else {
                            log::info!(
                                "[audio] Default device changed: {:?} -> {:?}, auto-switching",
                                current_device_name, new_default
                            );
                            finish_crossfade(&mut crossfade, &sink, &shared);
                            Self::handle_device_switch(
                                &mut _stream, &mut stream_handle, &mut sink,
                                &shared, &emit, &mut current_duration_ms,
                                &mut play_start, &mut accumulated_ms,
                                &mut is_playing, &mut preloaded,
                                &mut current_device_name, new_default,
                                ffmpeg_path.as_deref(), normalize,
                            );
                        }
                    }
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

            // Advance an active crossfade: equal-power volume ramp between the
            // outgoing sink and the current (incoming) sink.
            if crossfade.is_some() {
                let user_vol = read_state(&shared).playback.volume as f32;
                let done = {
                    let cf = crossfade.as_mut().expect("checked is_some above");
                    cf.elapsed_ms += cf.last_tick.elapsed().as_millis() as u64;
                    cf.last_tick = Instant::now();
                    let t = (cf.elapsed_ms as f32 / cf.duration_ms.max(1) as f32).min(1.0);
                    if t >= 1.0 || cf.old_sink.empty() {
                        true
                    } else {
                        sink.set_volume(user_vol * t.sqrt());
                        cf.old_sink.set_volume(user_vol * (1.0 - t).sqrt());
                        false
                    }
                };
                if done {
                    finish_crossfade(&mut crossfade, &sink, &shared);
                    log::debug!("[audio] Crossfade complete");
                }
            }

            // Try to preload next track when approaching end (~5s remaining, or
            // earlier so a crossfade has its source ready before the fade point)
            let preload_threshold_ms = 5000u64.max(crossfade_ms.saturating_add(3000));
            if preloaded.is_none() && crossfade.is_none() && current_duration_ms > 0 {
                let pos = accumulated_ms
                    + play_start.as_ref().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
                let remaining = current_duration_ms.saturating_sub(pos);
                if remaining < preload_threshold_ms {
                    preloaded = Self::try_preload_next(&shared, ffmpeg_path.as_deref());
                }
            }

            // Start a crossfade when entering the last `crossfade_ms` of the
            // current track. Exclusions (fall back to the normal gapless
            // transition): repeat-one, no preloaded next source, or a stale
            // preload that no longer matches the queue's next track.
            if crossfade_ms > 0 && crossfade.is_none() && preloaded.is_some() && current_duration_ms > 0 {
                let pos = accumulated_ms
                    + play_start.as_ref().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
                let remaining = current_duration_ms.saturating_sub(pos);
                let repeat = read_state(&shared).playback.repeat.clone();
                if repeat != RepeatMode::One && remaining > 250 && remaining <= crossfade_ms {
                    let preload_matches = {
                        let s = read_state(&shared);
                        s.queue
                            .peek_next(repeat == RepeatMode::All)
                            .zip(preloaded.as_ref())
                            .is_some_and(|(next, (pt, _, _))| next.id == pt.id)
                    };
                    if !preload_matches {
                        preloaded = None; // stale — re-preload on the next tick
                    } else {
                        match Self::make_sink(&stream_handle, 0.0) {
                            Err(e) => {
                                log::warn!("[audio] Crossfade sink creation failed, falling back to gapless: {}", e);
                            }
                            Ok(new_sink) => {
                                let (track, source, dur) = preloaded.take()
                                    .expect("preloaded checked above");
                                // Advance the queue to the incoming track
                                {
                                    let mut s = write_state(&shared);
                                    if s.queue.next().is_none() && s.playback.repeat == RepeatMode::All {
                                        s.queue.restart();
                                    }
                                }
                                append_with_gain(&new_sink, source, track.gain_db, normalize);
                                let fade_ms = crossfade_ms.min(remaining).max(1);
                                let old_sink = std::mem::replace(&mut sink, new_sink);
                                crossfade = Some(CrossfadeState {
                                    old_sink,
                                    duration_ms: fade_ms,
                                    elapsed_ms: 0,
                                    last_tick: Instant::now(),
                                });
                                // From here on, position/duration refer to the incoming track
                                current_duration_ms = if dur > 0 { dur } else {
                                    track.duration_ms.unwrap_or(0) as u64
                                };
                                accumulated_ms = 0;
                                play_start = Some(Instant::now());
                                {
                                    let mut s = write_state(&shared);
                                    s.playback.state = PlayerState::Playing;
                                    s.playback.current_track = Some(track.clone());
                                    s.playback.position_ms = 0;
                                    s.playback.duration_ms = current_duration_ms;
                                    s.playback.queue_length = s.queue.len();
                                    s.playback.queue_position = s.queue.position();
                                    emit(PlayerEvent::TrackChanged(Some(track)));
                                    emit(PlayerEvent::StateChanged(s.playback.clone()));
                                }
                                log::info!("[audio] Crossfade started ({}ms overlap)", fade_ms);
                            }
                        }
                    }
                }
            }

            // Update position for playing state
            let pos = accumulated_ms
                + play_start.as_ref().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);

            // Emit progress every ~500ms
            if last_progress_emit.elapsed() >= Duration::from_millis(450) {
                {
                    let mut s = write_state(&shared);
                    s.playback.position_ms = pos;
                }
                emit(PlayerEvent::Progress {
                    position_ms: pos,
                    duration_ms: current_duration_ms,
                });
                last_progress_emit = Instant::now();
            }

            // Check if track finished (sink is empty). `is_playing` is
            // guaranteed here (see `continue` above). Don't require a known
            // duration — tracks whose decoder reports none (duration 0) would
            // otherwise never auto-advance and stall the queue forever.
            if sink.empty() {
                log::info!("[audio] Track finished naturally");
                // If the incoming side of a crossfade ended before the fade did
                // (track shorter than the fade window), end the overlap now.
                finish_crossfade(&mut crossfade, &sink, &shared);
                let (next_exists, use_preloaded) = {
                    let mut s = write_state(&shared);
                    let repeat = s.playback.repeat.clone();
                    if repeat == RepeatMode::One {
                        (s.queue.current().is_some(), false)
                    } else {
                        let t = s.queue.next().cloned();
                        if t.is_none() && repeat == RepeatMode::All {
                            (s.queue.restart().is_some(), false)
                        } else if t.is_some() {
                            let matches_preload = preloaded.as_ref().is_some_and(|(pt, _, _)| {
                                s.queue.current().is_some_and(|ct| ct.id == pt.id)
                            });
                            (true, matches_preload)
                        } else {
                            (false, false)
                        }
                    }
                };

                if next_exists && use_preloaded {
                    log::info!("[audio] Gapless transition (preloaded)");
                    let (track, source, dur) = preloaded.take()
                        .expect("preloaded track must exist when use_preloaded is true");
                    current_duration_ms = if dur > 0 { dur } else {
                        track.duration_ms.unwrap_or(0) as u64
                    };
                    accumulated_ms = 0;
                    // Stop old sink, then fresh sink for the new track
                    sink.stop();
                    sink = make_sink_or_continue!(&stream_handle, sink.volume(), emit);
                    append_with_gain(&sink, source, track.gain_db, normalize);
                    play_start = Some(Instant::now());
                    {
                        let mut s = write_state(&shared);
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
                    Self::play_current(&mut sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms, &mut is_playing, ffmpeg_path.as_deref(), normalize);
                } else {
                    log::info!("[audio] Queue ended");
                    play_start = None;
                    accumulated_ms = 0;
                    current_duration_ms = 0;
                    is_playing = false;
                    preloaded = None;
                    // Stop old sink, fresh sink in paused state
                    sink.stop();
                    sink = make_sink_or_continue!(&stream_handle, sink.volume(), emit);
                    sink.pause();
                    let mut s = write_state(&shared);
                    s.playback.state = PlayerState::Stopped;
                    s.playback.position_ms = 0;
                    emit(PlayerEvent::StateChanged(s.playback.clone()));
                }
            }
        }
    }

    /// Handle auto-switching to a new default audio device.
    /// Extracted from the main loop to reduce nesting depth.
    #[allow(clippy::too_many_arguments)]
    fn handle_device_switch(
        _stream: &mut OutputStream,
        stream_handle: &mut OutputStreamHandle,
        sink: &mut Sink,
        shared: &Arc<RwLock<SharedState>>,
        emit: &dyn Fn(PlayerEvent),
        current_duration_ms: &mut u64,
        play_start: &mut Option<Instant>,
        accumulated_ms: &mut u64,
        is_playing: &mut bool,
        preloaded: &mut Option<(QueueTrack, PreloadedSource, u64)>,
        current_device_name: &mut Option<String>,
        new_default: Option<String>,
        ffmpeg_path: Option<&str>,
        normalize: bool,
    ) {
        let (new_stream, new_handle) = match OutputStream::try_default() {
            Ok(pair) => pair,
            Err(e) => {
                log::warn!("[audio] Failed to auto-switch to new default device: {}", e);
                *current_device_name = new_default;
                return;
            }
        };

        let vol = sink.volume();
        let was_playing = *is_playing;
        let position = *accumulated_ms
            + play_start.as_ref().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);

        sink.stop();
        *_stream = new_stream;
        *stream_handle = new_handle;
        *sink = match Self::make_sink(stream_handle, vol) {
            Ok(s) => s,
            Err(e) => {
                // The old stream is already gone and the old sink is stopped —
                // reset the playing state so the UI doesn't claim playback
                // until the next user command rebuilds a sink.
                log::error!("[audio] {}", e);
                *is_playing = false;
                *play_start = None;
                emit(PlayerEvent::Error(e));
                return;
            }
        };

        // If a track was playing, re-decode and seek to resume playback
        if was_playing {
            let current_track = read_state(shared).playback.current_track.clone();
            if let Some(ref track) = current_track {
                match Self::decode_track(&track.file_path, ffmpeg_path) {
                    Ok((source, dur)) => {
                        *current_duration_ms = if dur > 0 { dur } else { *current_duration_ms };
                        append_with_gain(sink, source, track.gain_db, normalize);
                        if position > 0 {
                            sink.try_seek(std::time::Duration::from_millis(position))
                                .unwrap_or_else(|e| log::warn!("[audio] Seek after device switch failed: {}", e));
                        }
                        *accumulated_ms = position;
                        *play_start = Some(Instant::now());
                        *is_playing = true;
                    }
                    Err(e) => {
                        log::warn!("[audio] Failed to re-decode after device switch: {}", e);
                        *is_playing = false;
                        sink.pause();
                    }
                }
            }
        } else {
            sink.pause();
        }

        *preloaded = None;
        *current_device_name = new_default;
        log::info!("[audio] Auto-switched to new default device: {:?}", current_device_name);
    }

    #[allow(clippy::too_many_arguments)]
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
        normalize: bool,
    ) {
        let track = read_state(shared).queue.current().cloned();

        if let Some(track) = track {
            log::info!("[audio] play_current: \"{}\" ({})", track.title, track.file_path);

            match Self::decode_track(&track.file_path, ffmpeg_path) {
                Ok((source, dur)) => {
                    // Use decoder duration, fall back to DB duration if unavailable
                    *current_duration_ms = if dur > 0 { dur } else {
                        track.duration_ms.unwrap_or(0) as u64
                    };
                    *accumulated_ms = 0;

                    // Stop old sink AFTER successful decode — stopping before decode
                    // would permanently kill the sink (rodio 0.20) and if decode then
                    // fails, the player is left with a dead sink and can't play anything.
                    let vol = sink.volume();
                    sink.stop();
                    *sink = match Self::make_sink(stream_handle, vol) {
                        Ok(s) => s,
                        Err(e) => {
                            log::error!("[audio] {}", e);
                            *is_playing = false;
                            emit(PlayerEvent::Error(e));
                            return;
                        }
                    };
                    append_with_gain(sink, source, track.gain_db, normalize);
                    // New sinks start un-paused, so playback begins immediately
                    *play_start = Some(Instant::now());
                    *is_playing = true;

                    {
                        let mut s = write_state(shared);
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
