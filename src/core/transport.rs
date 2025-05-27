use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum TransportEvent {
    Started { position: f64 },
    Stopped,
    Paused,
    PositionChanged { position: f64 },
    LoopRegionChanged { start: f64, end: f64 },
    TempoChanged { bpm: f64 },
}

pub trait TransportListener: Send + Sync {
    fn on_transport_event(&self, event: TransportEvent);
}

pub struct LoopRegion {
    pub start: f64,
    pub end: f64,
}

impl LoopRegion {
    pub fn new(start: f64, end: f64) -> Self {
        Self { start, end }
    }

    pub fn contains(&self, position: f64) -> bool {
        position >= self.start && position <= self.end
    }

    pub fn length(&self) -> f64 {
        self.end - self.start
    }

    pub fn formatted(&self) -> String {
        format!("{:.1}s - {:.1}s", self.start, self.end)
    }
}

pub struct Transport {
    // Playback state
    playing: AtomicBool,
    position: Arc<RwLock<f64>>,
    start_time: Arc<RwLock<Instant>>,
    pause_position: Arc<RwLock<f64>>,

    // Loop state
    loop_enabled: AtomicBool,
    loop_start: Arc<RwLock<f64>>,
    loop_end: Arc<RwLock<f64>>,

    // Tempo information
    bpm: Arc<RwLock<f64>>,

    // Listeners for transport events
    listeners: Arc<Mutex<Vec<Box<dyn TransportListener>>>>,
}

impl Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transport")
            .field("playing", &self.playing.load(Ordering::SeqCst))
            .field("position", &self.get_position())
            .field("loop_enabled", &self.loop_enabled.load(Ordering::SeqCst))
            .field("loop_start", &*self.loop_start.read().unwrap())
            .field("loop_end", &*self.loop_end.read().unwrap())
            .field("bpm", &*self.bpm.read().unwrap())
            .field("listeners_count", &self.listeners.lock().unwrap().len())
            .finish()
    }
}

impl Transport {
    pub fn new(initial_bpm: f64) -> Self {
        Self {
            bpm: Arc::new(RwLock::new(initial_bpm)),
            playing: AtomicBool::new(false),
            position: Arc::new(RwLock::new(0.0)),
            start_time: Arc::new(RwLock::new(Instant::now())),
            pause_position: Arc::new(RwLock::new(0.0)),
            loop_enabled: AtomicBool::new(false),
            loop_start: Arc::new(RwLock::new(0.0)),
            loop_end: Arc::new(RwLock::new(4.0)), // Default 4-bar loop
            listeners: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add_listener(&self, listener: Box<dyn TransportListener>) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.push(listener);
    }

    pub fn toggle_play_stop(&self) {
        if self.is_playing() {
            self.stop();
        } else {
            self.play();
        }
    }

    pub fn toggle_play_pause(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn play(&self) {
        let position = self.position.write().unwrap();
        let start_pos = *position;
        *self.start_time.write().unwrap() = Instant::now();
        *self.pause_position.write().unwrap() = *position;

        self.playing.store(true, Ordering::SeqCst);

        // Notify listeners
        self.notify_listeners(TransportEvent::Started {
            position: start_pos,
        });
    }

    pub fn stop(&self) {
        self.playing.store(false, Ordering::SeqCst);

        // Reset position to beginning
        *self.position.write().unwrap() = 0.0;

        // Notify listeners
        self.notify_listeners(TransportEvent::Stopped);
    }

    pub fn pause(&self) {
        // Only do something if we're currently playing
        if self.is_playing() {
            let current_pos = self.get_position();
            self.playing.store(false, Ordering::SeqCst);
            *self.pause_position.write().unwrap() = current_pos;

            // Notify listeners
            self.notify_listeners(TransportEvent::Paused);
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst)
    }

    pub fn seek_to(&self, position: f64) {
        let mut pos = self.position.write().unwrap();
        *pos = position;
        *self.pause_position.write().unwrap() = position;

        // If playing, reset start time
        if self.is_playing() {
            *self.start_time.write().unwrap() = Instant::now();
        }

        if self.is_loop_enabled() {
            // If we seeked outside the loop, disable the loop
            if position < *self.loop_start.read().unwrap()
                || position > *self.loop_end.read().unwrap()
            {
                self.loop_enabled.store(false, Ordering::SeqCst);
            }
        }

        // Notify listeners
        self.notify_listeners(TransportEvent::PositionChanged { position });
    }

    pub fn get_position(&self) -> f64 {
        if self.is_playing() {
            // Calculate current position based on elapsed time
            let start = *self.start_time.read().unwrap();
            let pause_pos = *self.pause_position.read().unwrap();
            let elapsed = start.elapsed().as_secs_f64();
            let current_pos = pause_pos + elapsed;

            // Handle looping
            if self.loop_enabled.load(Ordering::SeqCst) {
                let loop_start = *self.loop_start.read().unwrap();
                let loop_end = *self.loop_end.read().unwrap();

                if current_pos >= loop_end {
                    // Calculate wrapped position within loop
                    let loop_length = loop_end - loop_start;
                    if loop_length > 0.0 {
                        let wrapped_pos = loop_start + ((current_pos - loop_start) % loop_length);

                        // Update internal state to keep playback smooth
                        *self.start_time.write().unwrap() = Instant::now();
                        *self.pause_position.write().unwrap() = wrapped_pos;

                        return wrapped_pos;
                    }
                }
            }

            current_pos
        } else {
            *self.position.read().unwrap()
        }
    }

    pub fn set_loop_enabled(&self, enabled: bool) {
        self.loop_enabled.store(enabled, Ordering::SeqCst);
    }

    pub fn is_loop_enabled(&self) -> bool {
        self.loop_enabled.load(Ordering::SeqCst)
    }

    pub fn toggle_loop(&self) {
        let enabled = !self.is_loop_enabled();
        self.set_loop_enabled(enabled);

        // Notify listeners
        self.notify_listeners(TransportEvent::LoopRegionChanged {
            start: *self.loop_start.read().unwrap(),
            end: *self.loop_end.read().unwrap(),
        });
    }

    pub fn set_loop_start_to_current_time(&self) {
        self.set_loop_start(self.get_position());
    }
    pub fn set_loop_end_to_current_time(&self) {
        self.set_loop_end(self.get_position());
    }

    pub fn set_loop_start(&self, start: f64) {
        if start < *self.loop_end.read().unwrap() {
            *self.loop_start.write().unwrap() = start;

            // Notify listeners
            self.notify_listeners(TransportEvent::LoopRegionChanged {
                start,
                end: *self.loop_end.read().unwrap(),
            });
        }
    }

    pub fn set_loop_end(&self, end: f64) {
        if end > *self.loop_start.read().unwrap() {
            *self.loop_end.write().unwrap() = end;

            // Notify listeners
            self.notify_listeners(TransportEvent::LoopRegionChanged {
                start: *self.loop_start.read().unwrap(),
                end,
            });
        }
    }

    pub fn set_loop_region(&self, start: f64, end: f64) {
        if start < end {
            *self.loop_start.write().unwrap() = start;
            *self.loop_end.write().unwrap() = end;

            // Notify listeners
            self.notify_listeners(TransportEvent::LoopRegionChanged { start, end });
        }
    }

    pub fn get_loop_region(&self) -> LoopRegion {
        LoopRegion::new(
            *self.loop_start.read().unwrap(),
            *self.loop_end.read().unwrap(),
        )
    }

    pub fn set_bpm(&self, bpm: f64) {
        if bpm > 0.0 {
            *self.bpm.write().unwrap() = bpm;

            // Notify listeners
            self.notify_listeners(TransportEvent::TempoChanged { bpm });
        }
    }

    pub fn get_bpm(&self) -> f64 {
        *self.bpm.read().unwrap()
    }

    fn notify_listeners(&self, event: TransportEvent) {
        let listeners = self.listeners.lock().unwrap();
        for listener in listeners.iter() {
            println!("Notifying listener about event: {:?}", event);
            listener.on_transport_event(event.clone());
        }
    }

    pub fn on_ui_synced(&self) {
        // This can be called from the UI thread to sync the UI with transport
        // No need to do anything - get_position() will calculate the current position dynamically
    }
}

// Make TransportEvent cloneable to avoid ownership issues when notifying multiple listeners
impl Clone for TransportEvent {
    fn clone(&self) -> Self {
        match self {
            TransportEvent::Started { position } => TransportEvent::Started {
                position: *position,
            },
            TransportEvent::Stopped => TransportEvent::Stopped,
            TransportEvent::Paused => TransportEvent::Paused,
            TransportEvent::PositionChanged { position } => TransportEvent::PositionChanged {
                position: *position,
            },
            TransportEvent::LoopRegionChanged { start, end } => TransportEvent::LoopRegionChanged {
                start: *start,
                end: *end,
            },
            TransportEvent::TempoChanged { bpm } => TransportEvent::TempoChanged { bpm: *bpm },
        }
    }
}
