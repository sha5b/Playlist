use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
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
    RemoveFromQueue(usize), // order index
    ClearQueue,
    GetState,
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
    shared: Arc<Mutex<SharedState>>,
}

struct SharedState {
    playback: PlaybackState,
    queue: PlayQueue,
}

impl AudioEngine {
    pub fn new(event_callback: Box<dyn Fn(PlayerEvent) + Send + 'static>) -> Self {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();

        let shared = Arc::new(Mutex::new(SharedState {
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

        std::thread::spawn(move || {
            Self::audio_thread(cmd_rx, shared_clone, event_callback);
        });

        Self { cmd_tx, shared }
    }

    pub fn send(&self, cmd: PlayerCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn get_state(&self) -> PlaybackState {
        self.shared.lock().unwrap().playback.clone()
    }

    pub fn get_queue(&self) -> (Vec<QueueTrack>, Option<usize>) {
        let shared = self.shared.lock().unwrap();
        (shared.queue.get_ordered_tracks(), shared.queue.position())
    }

    fn audio_thread(
        cmd_rx: Receiver<PlayerCommand>,
        shared: Arc<Mutex<SharedState>>,
        emit: Box<dyn Fn(PlayerEvent) + Send>,
    ) {
        // Create audio output on this dedicated thread — OutputStream must stay alive
        let (_stream, stream_handle) = match OutputStream::try_default() {
            Ok(s) => s,
            Err(e) => {
                emit(PlayerEvent::Error(format!("Failed to open audio output: {}", e)));
                // Keep thread alive to process commands even without audio
                loop {
                    match cmd_rx.recv() {
                        Ok(_) => {}
                        Err(_) => return,
                    }
                }
            }
        };

        let sink = Sink::try_new(&stream_handle).unwrap();
        sink.set_volume(0.75);
        sink.pause(); // Start paused

        let mut current_duration_ms: u64 = 0;
        let mut play_start: Option<Instant> = None;
        let mut accumulated_ms: u64 = 0;
        let mut last_progress_emit = Instant::now();

        loop {
            // Non-blocking receive with timeout for progress updates
            match cmd_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(cmd) => {
                    match cmd {
                        PlayerCommand::Play { tracks, start_index } => {
                            {
                                let mut s = shared.lock().unwrap();
                                s.queue.set_tracks(tracks, start_index);
                            }
                            Self::play_current(&sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms);
                        }
                        PlayerCommand::PlaySingle(track) => {
                            {
                                let mut s = shared.lock().unwrap();
                                s.queue.set_tracks(vec![track], 0);
                            }
                            Self::play_current(&sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms);
                        }
                        PlayerCommand::Pause => {
                            sink.pause();
                            // Accumulate elapsed time
                            if let Some(start) = play_start.take() {
                                accumulated_ms += start.elapsed().as_millis() as u64;
                            }
                            {
                                let mut s = shared.lock().unwrap();
                                s.playback.state = PlayerState::Paused;
                                s.playback.position_ms = accumulated_ms;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                        }
                        PlayerCommand::Resume => {
                            if !sink.empty() {
                                sink.play();
                                play_start = Some(Instant::now());
                                {
                                    let mut s = shared.lock().unwrap();
                                    s.playback.state = PlayerState::Playing;
                                    emit(PlayerEvent::StateChanged(s.playback.clone()));
                                }
                            }
                        }
                        PlayerCommand::Stop => {
                            sink.stop();
                            play_start = None;
                            accumulated_ms = 0;
                            current_duration_ms = 0;
                            {
                                let mut s = shared.lock().unwrap();
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
                            let repeat = shared.lock().unwrap().playback.repeat.clone();
                            let next_track = {
                                let mut s = shared.lock().unwrap();
                                if repeat == RepeatMode::One {
                                    // Re-play current
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
                                Self::play_current(&sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms);
                            } else {
                                // Queue ended
                                sink.stop();
                                play_start = None;
                                accumulated_ms = 0;
                                {
                                    let mut s = shared.lock().unwrap();
                                    s.playback.state = PlayerState::Stopped;
                                    s.playback.position_ms = 0;
                                    emit(PlayerEvent::StateChanged(s.playback.clone()));
                                }
                            }
                        }
                        PlayerCommand::Prev => {
                            // If more than 3s in, restart current track
                            if accumulated_ms > 3000 || play_start.as_ref().map(|s| s.elapsed().as_millis() as u64 + accumulated_ms).unwrap_or(accumulated_ms) > 3000 {
                                Self::play_current(&sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms);
                            } else {
                                let has_prev = shared.lock().unwrap().queue.prev().is_some();
                                if has_prev {
                                    Self::play_current(&sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms);
                                } else {
                                    // At start, restart current
                                    Self::play_current(&sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms);
                                }
                            }
                        }
                        PlayerCommand::Seek(seconds) => {
                            // rodio Sink doesn't support seeking directly, so we re-create
                            let pos_ms = (seconds * 1000.0) as u64;
                            let file_path = shared.lock().unwrap().playback.current_track.as_ref().map(|t| t.file_path.clone());
                            if let Some(path) = file_path {
                                sink.stop();
                                if let Ok(file) = File::open(&path) {
                                    let reader = BufReader::new(file);
                                    if let Ok(source) = Decoder::new(reader) {
                                        // Try to skip to the desired position
                                        let skip_duration = Duration::from_millis(pos_ms);
                                        let source = source.skip_duration(skip_duration);
                                        sink.append(source);
                                        sink.play();
                                        accumulated_ms = pos_ms;
                                        play_start = Some(Instant::now());
                                        {
                                            let mut s = shared.lock().unwrap();
                                            s.playback.position_ms = pos_ms;
                                            s.playback.state = PlayerState::Playing;
                                            emit(PlayerEvent::StateChanged(s.playback.clone()));
                                        }
                                    }
                                }
                            }
                        }
                        PlayerCommand::SetVolume(vol) => {
                            let vol = vol.clamp(0.0, 1.0);
                            sink.set_volume(vol as f32);
                            {
                                let mut s = shared.lock().unwrap();
                                s.playback.volume = vol;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                        }
                        PlayerCommand::SetShuffle(shuffle) => {
                            {
                                let mut s = shared.lock().unwrap();
                                s.queue.set_shuffle(shuffle);
                                s.playback.shuffle = shuffle;
                                s.playback.queue_position = s.queue.position();
                                let tracks = s.queue.get_ordered_tracks();
                                let pos = s.queue.position();
                                emit(PlayerEvent::QueueUpdated { tracks, position: pos });
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                        }
                        PlayerCommand::SetRepeat(mode) => {
                            {
                                let mut s = shared.lock().unwrap();
                                s.playback.repeat = mode;
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                        }
                        PlayerCommand::AddToQueue(track) => {
                            {
                                let mut s = shared.lock().unwrap();
                                s.queue.add_track(track);
                                s.playback.queue_length = s.queue.len();
                                let tracks = s.queue.get_ordered_tracks();
                                let pos = s.queue.position();
                                emit(PlayerEvent::QueueUpdated { tracks, position: pos });
                            }
                        }
                        PlayerCommand::AddNext(track) => {
                            {
                                let mut s = shared.lock().unwrap();
                                s.queue.add_next(track);
                                s.playback.queue_length = s.queue.len();
                                let tracks = s.queue.get_ordered_tracks();
                                let pos = s.queue.position();
                                emit(PlayerEvent::QueueUpdated { tracks, position: pos });
                            }
                        }
                        PlayerCommand::RemoveFromQueue(order_idx) => {
                            {
                                let mut s = shared.lock().unwrap();
                                s.queue.remove_at_order_index(order_idx);
                                s.playback.queue_length = s.queue.len();
                                s.playback.queue_position = s.queue.position();
                                let tracks = s.queue.get_ordered_tracks();
                                let pos = s.queue.position();
                                emit(PlayerEvent::QueueUpdated { tracks, position: pos });
                            }
                        }
                        PlayerCommand::ClearQueue => {
                            // Keep current track but clear the rest
                            // For now, full clear + stop
                            sink.stop();
                            play_start = None;
                            accumulated_ms = 0;
                            {
                                let mut s = shared.lock().unwrap();
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
                        PlayerCommand::GetState => {
                            let s = shared.lock().unwrap();
                            emit(PlayerEvent::StateChanged(s.playback.clone()));
                        }
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // Check if track ended naturally
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return;
                }
            }

            // Update position for playing state
            if shared.lock().unwrap().playback.state == PlayerState::Playing {
                let pos = if let Some(start) = &play_start {
                    accumulated_ms + start.elapsed().as_millis() as u64
                } else {
                    accumulated_ms
                };

                {
                    let mut s = shared.lock().unwrap();
                    s.playback.position_ms = pos;
                }

                // Emit progress every ~500ms
                if last_progress_emit.elapsed() >= Duration::from_millis(450) {
                    let s = shared.lock().unwrap();
                    emit(PlayerEvent::Progress {
                        position_ms: pos,
                        duration_ms: s.playback.duration_ms,
                    });
                    last_progress_emit = Instant::now();
                }

                // Check if track finished (sink is empty)
                if sink.empty() && current_duration_ms > 0 {
                    // Track ended — auto-advance
                    let repeat = shared.lock().unwrap().playback.repeat.clone();
                    let next_exists = {
                        let mut s = shared.lock().unwrap();
                        if repeat == RepeatMode::One {
                            s.queue.current().is_some()
                        } else {
                            let t = s.queue.next().cloned();
                            if t.is_none() && repeat == RepeatMode::All {
                                s.queue.restart().is_some()
                            } else {
                                t.is_some()
                            }
                        }
                    };

                    if next_exists {
                        Self::play_current(&sink, &stream_handle, &shared, &emit, &mut current_duration_ms, &mut play_start, &mut accumulated_ms);
                    } else {
                        play_start = None;
                        accumulated_ms = 0;
                        current_duration_ms = 0;
                        let mut s = shared.lock().unwrap();
                        s.playback.state = PlayerState::Stopped;
                        s.playback.position_ms = 0;
                        emit(PlayerEvent::StateChanged(s.playback.clone()));
                    }
                }
            }
        }
    }

    fn play_current(
        sink: &Sink,
        _stream_handle: &OutputStreamHandle,
        shared: &Arc<Mutex<SharedState>>,
        emit: &dyn Fn(PlayerEvent),
        current_duration_ms: &mut u64,
        play_start: &mut Option<Instant>,
        accumulated_ms: &mut u64,
    ) {
        let track = shared.lock().unwrap().queue.current().cloned();

        if let Some(track) = track {
            sink.stop();

            match File::open(&track.file_path) {
                Ok(file) => {
                    let reader = BufReader::new(file);
                    match Decoder::new(reader) {
                        Ok(source) => {
                            *current_duration_ms = track.duration_ms.unwrap_or(0) as u64;
                            *accumulated_ms = 0;

                            sink.append(source);
                            sink.play();
                            *play_start = Some(Instant::now());

                            {
                                let mut s = shared.lock().unwrap();
                                s.playback.state = PlayerState::Playing;
                                s.playback.current_track = Some(track.clone());
                                s.playback.position_ms = 0;
                                s.playback.duration_ms = *current_duration_ms;
                                s.playback.queue_length = s.queue.len();
                                s.playback.queue_position = s.queue.position();
                                emit(PlayerEvent::TrackChanged(Some(track)));
                                emit(PlayerEvent::StateChanged(s.playback.clone()));
                            }
                        }
                        Err(e) => {
                            emit(PlayerEvent::Error(format!("Failed to decode audio: {}", e)));
                        }
                    }
                }
                Err(e) => {
                    emit(PlayerEvent::Error(format!("Failed to open file: {}", e)));
                }
            }
        }
    }
}
