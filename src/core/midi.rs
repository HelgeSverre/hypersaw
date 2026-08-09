use midly::{MetaMessage, MidiMessage as MidlyMessage, TrackEventKind};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use uuid::Uuid;

// Unique identifier for MIDI notes and events
pub type EventID = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MidiMessage {
    // Note messages
    NoteOn {
        channel: u8,
        key: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        key: u8,
        velocity: u8,
    },

    // Control messages
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    PitchBend {
        channel: u8,
        value: i16, // -8192 to +8191
    },
    Aftertouch {
        channel: u8,
        key: u8,
        pressure: u8,
    },

    // System messages
    SysEx(Vec<u8>),
    MidiClock,
    MidiStart,
    MidiStop,
    MidiContinue,
}

impl MidiMessage {
    pub fn with_channel(mut self, channel: u8) -> Self {
        let channel = channel.min(15);
        match &mut self {
            Self::NoteOn { channel: value, .. }
            | Self::NoteOff { channel: value, .. }
            | Self::ControlChange { channel: value, .. }
            | Self::ProgramChange { channel: value, .. }
            | Self::PitchBend { channel: value, .. }
            | Self::Aftertouch { channel: value, .. } => *value = channel,
            Self::SysEx(_)
            | Self::MidiClock
            | Self::MidiStart
            | Self::MidiStop
            | Self::MidiContinue => {}
        }
        self
    }

    /// Return the zero-based MIDI channel for channel-scoped messages.
    pub fn channel(&self) -> Option<u8> {
        match self {
            Self::NoteOn { channel, .. }
            | Self::NoteOff { channel, .. }
            | Self::ControlChange { channel, .. }
            | Self::ProgramChange { channel, .. }
            | Self::PitchBend { channel, .. }
            | Self::Aftertouch { channel, .. } => Some(*channel),
            Self::SysEx(_)
            | Self::MidiClock
            | Self::MidiStart
            | Self::MidiStop
            | Self::MidiContinue => None,
        }
    }
}

pub fn midi_channel_index(display_channel: u8) -> u8 {
    display_channel.saturating_sub(1).min(15)
}

#[cfg(test)]
mod message_tests {
    use super::{MidiEvent, MidiEventStore, MidiMessage, Note};

    fn note(id: &str, start_time: f64, duration: f64, key: u8) -> Note {
        Note {
            id: id.to_string(),
            channel: 0,
            key,
            velocity: 100,
            start_time,
            duration,
            start_tick: (start_time * 960.0) as u32,
            duration_ticks: (duration * 960.0) as u32,
        }
    }

    #[test]
    fn with_channel_rewrites_channel_messages() {
        let message = MidiMessage::NoteOn {
            channel: 0,
            key: 60,
            velocity: 100,
        }
        .with_channel(9);
        assert_eq!(
            message,
            MidiMessage::NoteOn {
                channel: 9,
                key: 60,
                velocity: 100
            }
        );
    }

    #[test]
    fn with_channel_clamps_to_midi_range() {
        let message = MidiMessage::ProgramChange {
            channel: 0,
            program: 1,
        }
        .with_channel(16);
        assert_eq!(
            message,
            MidiMessage::ProgramChange {
                channel: 15,
                program: 1
            }
        );
    }

    #[test]
    fn display_channels_are_converted_to_zero_based_midi_channels() {
        assert_eq!(super::midi_channel_index(1), 0);
        assert_eq!(super::midi_channel_index(16), 15);
    }

    #[test]
    fn event_range_includes_an_event_at_its_start_boundary() {
        let mut store = MidiEventStore::new(480);
        store.add_note(Note {
            id: "note".to_string(),
            channel: 0,
            key: 60,
            velocity: 100,
            start_time: 0.0,
            duration: 1.0,
            start_tick: 0,
            duration_ticks: 480,
        });
        let events = store.get_events_in_range(0.0, 0.5);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].message, MidiMessage::NoteOn { .. }));
    }

    #[test]
    fn imported_tick_zero_tempo_replaces_the_default_before_time_conversion() {
        let smf = midly::Smf {
            header: midly::Header {
                format: midly::Format::SingleTrack,
                timing: midly::Timing::Metrical(480.into()),
            },
            tracks: vec![vec![
                midly::TrackEvent {
                    delta: 0.into(),
                    kind: midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(1_000_000.into())),
                },
                midly::TrackEvent {
                    delta: 480.into(),
                    kind: midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(500_000.into())),
                },
                midly::TrackEvent {
                    delta: 0.into(),
                    kind: midly::TrackEventKind::Midi {
                        channel: 0.into(),
                        message: midly::MidiMessage::NoteOn {
                            key: 60.into(),
                            vel: 100.into(),
                        },
                    },
                },
                midly::TrackEvent {
                    delta: 480.into(),
                    kind: midly::TrackEventKind::Midi {
                        channel: 0.into(),
                        message: midly::MidiMessage::NoteOff {
                            key: 60.into(),
                            vel: 0.into(),
                        },
                    },
                },
            ]],
        };
        let path =
            std::env::temp_dir().join(format!("tempo-regression-{}.mid", uuid::Uuid::new_v4()));
        let mut file = std::fs::File::create(&path).unwrap();
        smf.write_std(&mut file).unwrap();

        let store = MidiEventStore::load_from_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        let note = store.get_notes().next().unwrap();
        assert_eq!(store.tempo_map.len(), 2);
        assert_eq!(note.start_time, 1.0);
        assert_eq!(note.duration, 0.5);
    }

    #[test]
    fn updating_note_velocity_updates_its_note_on_event() {
        let mut store = MidiEventStore::new(480);
        store.add_note(Note {
            id: "note".to_string(),
            channel: 0,
            key: 60,
            velocity: 64,
            start_time: 0.0,
            duration: 1.0,
            start_tick: 0,
            duration_ticks: 480,
        });

        store.update_note_velocity("note", 110);

        assert_eq!(store.get_note("note").map(|note| note.velocity), Some(110));
        assert!(matches!(
            store.event_data["note_on"].message,
            MidiMessage::NoteOn { velocity: 110, .. }
        ));
    }

    #[test]
    fn merge_from_preserves_existing_notes_and_offsets_new_material() {
        let mut target = MidiEventStore::new(480);
        target.add_note(note("existing", 0.0, 0.5, 60));
        let mut source = MidiEventStore::new(480);
        source.add_note(note("source", 0.25, 0.5, 64));

        target.merge_from(&source, 2.0);

        let mut notes: Vec<_> = target.get_notes().collect();
        notes.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].start_time, 0.0);
        assert_eq!(notes[1].start_time, 2.25);
        assert_ne!(notes[1].id, "source");
    }

    #[test]
    fn replace_range_trims_crossing_notes_and_preserves_outside_events() {
        let mut target = MidiEventStore::new(480);
        target.add_note(note("crossing", 0.0, 4.0, 60));
        target.add_event(MidiEvent {
            id: "before".to_string(),
            time: 0.5,
            tick: 480,
            message: MidiMessage::ControlChange {
                channel: 0,
                controller: 1,
                value: 10,
            },
        });
        target.add_event(MidiEvent {
            id: "inside".to_string(),
            time: 1.5,
            tick: 1440,
            message: MidiMessage::ControlChange {
                channel: 0,
                controller: 1,
                value: 20,
            },
        });
        let mut replacement = MidiEventStore::new(480);
        replacement.add_note(note("replacement", 0.0, 0.5, 72));

        target.replace_range_from(1.0, 2.0, &replacement);

        let mut notes: Vec<_> = target.get_notes().collect();
        notes.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        assert_eq!(notes.len(), 3);
        assert_eq!((notes[0].start_time, notes[0].duration), (0.0, 1.0));
        assert_eq!((notes[1].start_time, notes[1].duration), (1.0, 0.5));
        assert_eq!((notes[2].start_time, notes[2].duration), (2.0, 2.0));
        assert!(target.event_data.contains_key("before"));
        assert!(!target.event_data.contains_key("inside"));
    }

    #[test]
    fn extract_range_splits_notes_at_loop_boundaries() {
        let mut source = MidiEventStore::new(480);
        source.add_note(note("crossing", 0.75, 0.5, 60));

        let first = source.extract_range(0.0, 1.0);
        let second = source.extract_range(1.0, 2.0);
        let first_note = first.get_notes().next().unwrap();
        let second_note = second.get_notes().next().unwrap();

        assert_eq!((first_note.start_time, first_note.duration), (0.75, 0.25));
        assert_eq!((second_note.start_time, second_note.duration), (0.0, 0.25));
    }
}

// A single MIDI event with timing information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MidiEvent {
    pub id: EventID,
    pub time: f64, // Time in seconds
    pub tick: u32, // Time in ticks (for grid alignment)
    pub message: MidiMessage,
}

// A note representation that connects note-on and note-off events
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub id: EventID,
    pub channel: u8,
    pub key: u8,
    pub velocity: u8,
    pub start_time: f64,
    pub duration: f64,
    pub start_tick: u32,
    pub duration_ticks: u32,
}

// Efficient storage and lookup of MIDI data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MidiEventStore {
    // Events sorted by time for playback
    events_by_time: BTreeMap<OrderedFloat<f64>, Vec<EventID>>,

    // Events sorted by tick for grid operations
    events_by_tick: BTreeMap<u32, Vec<EventID>>,

    // Quick lookup of event data by ID
    event_data: HashMap<EventID, MidiEvent>,

    // Notes for piano roll display/editing
    notes: HashMap<EventID, Note>,

    // Track tempo changes
    tempo_map: Vec<TempoChange>,

    // Time signature changes
    time_signatures: Vec<TimeSignature>,

    ppq: u32, // Pulses per quarter note (time resolution)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TempoChange {
    pub tick: u32,
    pub tempo: u32, // Microseconds per quarter note
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimeSignature {
    pub tick: u32,
    pub numerator: u8,
    pub denominator: u8,
}

impl MidiEventStore {
    pub fn new(ppq: u32) -> Self {
        Self {
            events_by_time: BTreeMap::new(),
            events_by_tick: BTreeMap::new(),
            event_data: HashMap::new(),
            notes: HashMap::new(),
            tempo_map: vec![TempoChange {
                tick: 0,
                tempo: 500_000,
            }],
            time_signatures: vec![TimeSignature {
                tick: 0,
                numerator: 4,
                denominator: 4,
            }],
            ppq,
        }
    }

    pub fn add_event(&mut self, event: MidiEvent) {
        let id = event.id.clone();
        let time = OrderedFloat(event.time); // Convert f64 to OrderedFloat
        let tick = event.tick;

        self.events_by_time
            .entry(time)
            .or_default()
            .push(id.clone());
        self.events_by_tick
            .entry(tick)
            .or_default()
            .push(id.clone());
        self.event_data.insert(id, event);
    }

    pub fn add_note(&mut self, note: Note) {
        // Create note-on event
        let note_on = MidiEvent {
            id: format!("{}_on", note.id),
            time: note.start_time,
            tick: note.start_tick,
            message: MidiMessage::NoteOn {
                channel: note.channel,
                key: note.key,
                velocity: note.velocity,
            },
        };

        // Create note-off event
        let note_off = MidiEvent {
            id: format!("{}_off", note.id),
            time: note.start_time + note.duration,
            tick: note.start_tick + note.duration_ticks,
            message: MidiMessage::NoteOff {
                channel: note.channel,
                key: note.key,
                velocity: 0,
            },
        };

        // Add both events
        self.add_event(note_on);
        self.add_event(note_off);
        self.notes.insert(note.id.clone(), note);
    }

    pub fn get_events_in_range(&self, start_time: f64, end_time: f64) -> Vec<&MidiEvent> {
        self.events_by_time
            .range(OrderedFloat(start_time)..OrderedFloat(end_time))
            .flat_map(|(_, ids)| ids.iter())
            .filter_map(|id| self.event_data.get(id))
            .collect()
    }

    pub fn get_notes_in_range(&self, start_time: f64, end_time: f64) -> Vec<&Note> {
        self.notes
            .values()
            .filter(|note| {
                let note_end = note.start_time + note.duration;
                note.start_time < end_time && note_end > start_time
            })
            .collect()
    }
}

impl MidiEventStore {
    // Accessors
    pub fn get_last_event_time(&self) -> Option<f64> {
        self.events_by_time.keys().last().map(|k| k.0)
    }

    pub fn get_events(&self) -> impl Iterator<Item = &MidiEvent> {
        self.event_data.values()
    }

    pub fn get_notes(&self) -> impl Iterator<Item = &Note> {
        self.notes.values()
    }

    pub fn get_note(&self, note_id: &str) -> Option<&Note> {
        self.notes.get(note_id)
    }

    pub fn get_note_mut(&mut self, note_id: &str) -> Option<&mut Note> {
        self.notes.get_mut(note_id)
    }

    /// Merge another store at a seconds-based offset using fresh event/note IDs.
    pub fn merge_from(&mut self, source: &Self, offset_seconds: f64) {
        let mut notes: Vec<_> = source.notes.values().cloned().collect();
        notes.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        for note in notes {
            let shifted_end = note.start_time + note.duration + offset_seconds;
            if shifted_end <= 0.0 {
                continue;
            }

            let shifted_start = (note.start_time + offset_seconds).max(0.0);
            self.add_note_with_fresh_id(
                note.channel,
                note.key,
                note.velocity,
                shifted_start,
                shifted_end - shifted_start,
            );
        }

        let note_event_ids = source.note_event_ids();
        let mut events: Vec<_> = source
            .event_data
            .values()
            .filter(|event| !note_event_ids.contains(&event.id))
            .cloned()
            .collect();
        events.sort_by(|left, right| left.time.total_cmp(&right.time));
        for event in events {
            let shifted_time = event.time + offset_seconds;
            if shifted_time < 0.0 {
                continue;
            }
            self.add_event(MidiEvent {
                id: Uuid::new_v4().to_string(),
                time: shifted_time,
                tick: self.time_to_tick(shifted_time),
                message: event.message,
            });
        }
    }

    /// Return material intersecting `[start_time, end_time)`, shifted to zero.
    /// Notes crossing either boundary are trimmed to the requested interval.
    pub fn extract_range(&self, start_time: f64, end_time: f64) -> Self {
        let mut extracted = self.empty_like();
        if !start_time.is_finite() || !end_time.is_finite() || end_time <= start_time {
            return extracted;
        }

        let mut notes: Vec<_> = self.notes.values().collect();
        notes.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        for note in notes {
            let note_end = note.start_time + note.duration;
            let clipped_start = note.start_time.max(start_time);
            let clipped_end = note_end.min(end_time);
            if clipped_end <= clipped_start {
                continue;
            }
            extracted.add_note_with_fresh_id(
                note.channel,
                note.key,
                note.velocity,
                clipped_start - start_time,
                clipped_end - clipped_start,
            );
        }

        let note_event_ids = self.note_event_ids();
        let mut events: Vec<_> = self
            .event_data
            .values()
            .filter(|event| {
                !note_event_ids.contains(&event.id)
                    && event.time >= start_time
                    && event.time < end_time
            })
            .cloned()
            .collect();
        events.sort_by(|left, right| left.time.total_cmp(&right.time));
        for event in events {
            let time = event.time - start_time;
            extracted.add_event(MidiEvent {
                id: Uuid::new_v4().to_string(),
                time,
                tick: extracted.time_to_tick(time),
                message: event.message,
            });
        }

        extracted
    }

    /// Replace only `[start_time, end_time)` and preserve/trim surrounding data.
    pub fn replace_range_from(&mut self, start_time: f64, end_time: f64, replacement: &Self) {
        if !start_time.is_finite() || !end_time.is_finite() || end_time <= start_time {
            return;
        }

        let overlapping: Vec<_> = self
            .notes
            .values()
            .filter(|note| {
                note.start_time < end_time && note.start_time + note.duration > start_time
            })
            .cloned()
            .collect();

        for note in overlapping {
            let note_end = note.start_time + note.duration;
            self.delete_note(&note.id);
            if note.start_time < start_time {
                self.add_note_with_fresh_id(
                    note.channel,
                    note.key,
                    note.velocity,
                    note.start_time,
                    start_time - note.start_time,
                );
            }
            if note_end > end_time {
                self.add_note_with_fresh_id(
                    note.channel,
                    note.key,
                    note.velocity,
                    end_time,
                    note_end - end_time,
                );
            }
        }

        let note_event_ids = self.note_event_ids();
        let event_ids: Vec<_> = self
            .event_data
            .values()
            .filter(|event| {
                !note_event_ids.contains(&event.id)
                    && event.time >= start_time
                    && event.time < end_time
            })
            .map(|event| event.id.clone())
            .collect();
        for event_id in event_ids {
            self.delete_event(&event_id);
        }

        self.merge_from(replacement, start_time);
    }

    fn empty_like(&self) -> Self {
        Self {
            events_by_time: BTreeMap::new(),
            events_by_tick: BTreeMap::new(),
            event_data: HashMap::new(),
            notes: HashMap::new(),
            tempo_map: self.tempo_map.clone(),
            time_signatures: self.time_signatures.clone(),
            ppq: self.ppq,
        }
    }

    fn note_event_ids(&self) -> HashSet<EventID> {
        self.notes
            .keys()
            .flat_map(|note_id| [format!("{note_id}_on"), format!("{note_id}_off")])
            .collect()
    }

    fn add_note_with_fresh_id(
        &mut self,
        channel: u8,
        key: u8,
        velocity: u8,
        start_time: f64,
        duration: f64,
    ) {
        let duration = duration.max(1.0 / 1000.0);
        self.add_note(Note {
            id: Uuid::new_v4().to_string(),
            channel,
            key,
            velocity,
            start_time,
            duration,
            start_tick: self.time_to_tick(start_time),
            duration_ticks: self.time_to_tick(duration),
        });
    }

    fn delete_event(&mut self, event_id: &str) {
        for event_ids in self.events_by_time.values_mut() {
            event_ids.retain(|id| id != event_id);
        }
        self.events_by_time.retain(|_, ids| !ids.is_empty());
        for event_ids in self.events_by_tick.values_mut() {
            event_ids.retain(|id| id != event_id);
        }
        self.events_by_tick.retain(|_, ids| !ids.is_empty());
        self.event_data.remove(event_id);
    }

    pub fn rebuild_note_maps(&mut self) {
        // Clear the existing maps
        self.events_by_time.clear();
        self.events_by_tick.clear();
        self.event_data.clear();

        // Rebuild from notes
        let notes: Vec<Note> = self.notes.values().cloned().collect();
        for note in notes {
            self.add_note(note);
        }
    }

    // Time conversion methods
    pub fn tick_to_time(&self, tick: u32) -> f64 {
        // Accumulate time across all tempo segments up to the target tick
        let mut accumulated_time = 0.0;
        let mut current_tick = 0u32;

        for i in 0..self.tempo_map.len() {
            let current_tempo = &self.tempo_map[i];
            let next_tick = if i + 1 < self.tempo_map.len() {
                self.tempo_map[i + 1].tick.min(tick)
            } else {
                tick
            };

            if next_tick <= current_tick {
                break;
            }

            // Calculate time for this segment
            let tick_delta = next_tick - current_tick;
            let seconds_per_tick = current_tempo.tempo as f64 / (self.ppq as f64 * 1_000_000.0);
            accumulated_time += tick_delta as f64 * seconds_per_tick;

            current_tick = next_tick;

            if current_tick >= tick {
                break;
            }
        }

        accumulated_time
    }

    pub fn time_to_tick(&self, time: f64) -> u32 {
        // Accumulate ticks across tempo segments until we reach the target time
        let mut accumulated_time = 0.0;
        let mut accumulated_ticks = 0u32;

        for i in 0..self.tempo_map.len() {
            let current_tempo = &self.tempo_map[i];
            let next_tempo_tick = if i + 1 < self.tempo_map.len() {
                self.tempo_map[i + 1].tick
            } else {
                u32::MAX
            };

            let seconds_per_tick = current_tempo.tempo as f64 / (self.ppq as f64 * 1_000_000.0);
            let ticks_per_second = 1.0 / seconds_per_tick;

            // How much time is available in this tempo segment?
            let ticks_in_segment = next_tempo_tick - accumulated_ticks;
            let time_in_segment = ticks_in_segment as f64 * seconds_per_tick;

            if accumulated_time + time_in_segment >= time {
                // Target time is in this segment
                let remaining_time = time - accumulated_time;
                let remaining_ticks = (remaining_time * ticks_per_second) as u32;
                return accumulated_ticks + remaining_ticks;
            }

            // Move to next segment
            accumulated_time += time_in_segment;
            accumulated_ticks = next_tempo_tick;
        }

        // If we get here, use the last tempo
        let last_tempo = &self.tempo_map[self.tempo_map.len() - 1];
        let seconds_per_tick = last_tempo.tempo as f64 / (self.ppq as f64 * 1_000_000.0);
        let ticks_per_second = 1.0 / seconds_per_tick;
        let remaining_time = time - accumulated_time;
        accumulated_ticks + (remaining_time * ticks_per_second) as u32
    }

    pub fn delete_note(&mut self, note_id: &str) {
        // First collect all the IDs we need to remove
        let on_id = format!("{}_on", note_id);
        let off_id = format!("{}_off", note_id);

        // Remove from events_by_time
        for events in self.events_by_time.values_mut() {
            events.retain(|id| id != &on_id && id != &off_id);
        }
        // Clean up empty entries
        self.events_by_time.retain(|_, events| !events.is_empty());

        // Remove from events_by_tick
        for events in self.events_by_tick.values_mut() {
            events.retain(|id| id != &on_id && id != &off_id);
        }
        // Clean up empty entries
        self.events_by_tick.retain(|_, events| !events.is_empty());

        // Remove from event_data
        self.event_data.remove(&on_id);
        self.event_data.remove(&off_id);

        // Remove the note itself
        self.notes.remove(note_id);
    }

    pub fn update_note(&mut self, note_id: &str, new_start: f64, new_duration: f64) {
        // First get a clone of the note we want to update
        let mut updated_note = if let Some(note) = self.notes.get(note_id) {
            note.clone()
        } else {
            return;
        };

        // Calculate new timings
        let start_tick = self.time_to_tick(new_start);
        let duration_ticks = self.time_to_tick(new_duration);

        // Update the note's timing
        updated_note.start_time = new_start;
        updated_note.duration = new_duration;
        updated_note.start_tick = start_tick;
        updated_note.duration_ticks = duration_ticks;

        // Remove old events
        self.delete_note(note_id);

        // Add updated note
        self.add_note(updated_note);
    }

    pub fn update_note_velocity(&mut self, note_id: &str, new_velocity: u8) {
        if let Some(note) = self.notes.get_mut(note_id) {
            note.velocity = new_velocity;
        } else {
            return;
        }

        if let Some(MidiEvent {
            message: MidiMessage::NoteOn { velocity, .. },
            ..
        }) = self.event_data.get_mut(&format!("{note_id}_on"))
        {
            *velocity = new_velocity;
        }
    }

    /// Sort imported tempo changes and retain the final tempo at each tick.
    ///
    /// A store starts with the default 120 BPM event at tick zero. MIDI files
    /// commonly carry their own tick-zero tempo, so deduplicating is required
    /// before converting ticks to time; otherwise the zero-length first segment
    /// causes `tick_to_time` to stop immediately.
    fn normalize_tempo_map(&mut self) {
        self.tempo_map.sort_by_key(|change| change.tick);

        let mut normalized = Vec::with_capacity(self.tempo_map.len() + 1);
        for change in self.tempo_map.drain(..) {
            if let Some(previous) = normalized
                .last_mut()
                .filter(|previous: &&mut TempoChange| previous.tick == change.tick)
            {
                *previous = change;
            } else {
                normalized.push(change);
            }
        }

        if normalized.first().is_none_or(|change| change.tick != 0) {
            normalized.insert(
                0,
                TempoChange {
                    tick: 0,
                    tempo: 500_000,
                },
            );
        }

        self.tempo_map = normalized;
    }

    pub fn move_note(&mut self, note_id: &str, delta_time: f64, delta_pitch: i8) {
        // First get a clone of the note we want to update
        let mut updated_note = if let Some(note) = self.notes.get(note_id) {
            note.clone()
        } else {
            return;
        };

        // Update timing
        let new_start = (updated_note.start_time + delta_time).max(0.0);
        let start_tick = self.time_to_tick(new_start);

        // Update pitch
        let new_pitch = (updated_note.key as i16 + delta_pitch as i16).clamp(0, 127) as u8;

        // Apply updates to the cloned note
        updated_note.start_time = new_start;
        updated_note.start_tick = start_tick;
        updated_note.key = new_pitch;

        // Remove old events
        self.delete_note(note_id);

        // Add updated note
        self.add_note(updated_note);
    }
    // Load from MIDI file
    pub fn load_from_file(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let mut file = File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        let smf = midly::Smf::parse(&buffer)?;
        let ppq = match smf.header.timing {
            midly::Timing::Metrical(ticks) => ticks.as_int() as u32,
            _ => return Err("Unsupported timing format".into()),
        };

        let mut store = MidiEventStore::new(ppq);

        // Tempo events govern timing across every track, so collect and
        // normalize them before constructing timestamped events.
        for track in &smf.tracks {
            let mut running_tick = 0;
            for event in track {
                running_tick += event.delta.as_int();
                if let TrackEventKind::Meta(MetaMessage::Tempo(tempo)) = event.kind {
                    store.tempo_map.push(TempoChange {
                        tick: running_tick,
                        tempo: tempo.as_int(),
                    });
                }
            }
        }
        store.normalize_tempo_map();

        // Process each track
        for track in smf.tracks {
            let mut running_tick = 0;
            let mut active_notes: HashMap<(u8, u8), (EventID, u32, u8)> = HashMap::new(); // (channel, key) -> (id, start_tick, velocity)

            for event in track {
                running_tick += event.delta.as_int();

                match event.kind {
                    TrackEventKind::Midi { message, channel } => {
                        match message {
                            MidlyMessage::NoteOn { key, vel } => {
                                if vel.as_int() > 0 {
                                    // Note ON
                                    let id = Uuid::new_v4().to_string();
                                    active_notes.insert(
                                        (channel.as_int(), key.as_int()),
                                        (id, running_tick, vel.as_int()),
                                    );
                                } else {
                                    // Note OFF (velocity 0)
                                    Self::handle_note_off(
                                        &mut store,
                                        channel.as_int(),
                                        key.as_int(),
                                        running_tick,
                                        &mut active_notes,
                                    );
                                }
                            }
                            MidlyMessage::NoteOff { key, vel } => {
                                Self::handle_note_off(
                                    &mut store,
                                    channel.as_int(),
                                    key.as_int(),
                                    running_tick,
                                    &mut active_notes,
                                );
                            }
                            // Handle other MIDI messages
                            msg => {
                                if let Some(midi_msg) =
                                    Self::convert_midly_message(msg, channel.as_int())
                                {
                                    store.add_event(MidiEvent {
                                        id: Uuid::new_v4().to_string(),
                                        time: store.tick_to_time(running_tick),
                                        tick: running_tick,
                                        message: midi_msg,
                                    });
                                }
                            }
                        }
                    }
                    TrackEventKind::Meta(meta_msg) => match meta_msg {
                        MetaMessage::Tempo(_) => {}
                        MetaMessage::TimeSignature(num, denom, _, _) => {
                            store.time_signatures.push(TimeSignature {
                                tick: running_tick,
                                numerator: num,
                                denominator: 2u8.pow(denom as u32),
                            });
                        }
                        _ => {}
                    },
                    TrackEventKind::SysEx(data) => {
                        store.add_event(MidiEvent {
                            id: Uuid::new_v4().to_string(),
                            time: store.tick_to_time(running_tick),
                            tick: running_tick,
                            message: MidiMessage::SysEx(data.to_vec()),
                        });
                    }
                    _ => {}
                }
            }

            // Handle any still-active notes at track end
            for ((channel, key), (id, start_tick, velocity)) in active_notes {
                store.add_note(Note {
                    id,
                    channel,
                    key,
                    velocity,
                    start_time: store.tick_to_time(start_tick),
                    duration: store.tick_to_time(running_tick) - store.tick_to_time(start_tick),
                    start_tick,
                    duration_ticks: running_tick - start_tick,
                });
            }
        }

        Ok(store)
    }

    // Save to MIDI file
    pub fn save_to_file(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let mut tracks = Vec::new();
        let mut events: Vec<(u32, MidiMessage)> = Vec::new();

        // Collect all events sorted by tick
        for (tick, event_ids) in &self.events_by_tick {
            for id in event_ids {
                if let Some(event) = self.event_data.get(id) {
                    events.push((*tick, event.message.clone()));
                }
            }
        }

        // Sort by tick
        events.sort_by_key(|(tick, _)| *tick);

        // Convert to MIDI track
        let mut track = Vec::new();
        let mut last_tick = 0;

        for (tick, msg) in events {
            let delta = tick - last_tick;
            last_tick = tick;

            // Convert our message type to midly's message type
            if let Some((channel, midi_msg)) = Self::convert_to_midly_message(&msg) {
                track.push(midly::TrackEvent {
                    delta: delta.into(),
                    kind: TrackEventKind::Midi {
                        channel: channel.into(),
                        message: midi_msg,
                    },
                });
            }
        }

        tracks.push(track);

        // Create and write SMF
        let smf = midly::Smf {
            header: midly::Header {
                format: midly::Format::SingleTrack,
                timing: midly::Timing::Metrical((self.ppq as u16).into()), // Convert to u16 first
            },
            tracks,
        };

        let mut file = File::create(path)?;
        smf.write_std(&mut file)?;

        Ok(())
    }

    fn handle_note_off(
        store: &mut MidiEventStore,
        channel: u8,
        key: u8,
        end_tick: u32,
        active_notes: &mut HashMap<(u8, u8), (EventID, u32, u8)>,
    ) {
        if let Some((id, start_tick, velocity)) = active_notes.remove(&(channel, key)) {
            store.add_note(Note {
                id,
                channel,
                key,
                velocity,
                start_time: store.tick_to_time(start_tick),
                duration: store.tick_to_time(end_tick) - store.tick_to_time(start_tick),
                start_tick,
                duration_ticks: end_tick - start_tick,
            });
        }
    }

    fn convert_midly_message(msg: MidlyMessage, channel: u8) -> Option<MidiMessage> {
        match msg {
            MidlyMessage::Controller { controller, value } => Some(MidiMessage::ControlChange {
                channel,
                controller: controller.as_int(),
                value: value.as_int(),
            }),
            MidlyMessage::ProgramChange { program } => Some(MidiMessage::ProgramChange {
                channel,
                program: program.as_int(),
            }),
            MidlyMessage::PitchBend { bend } => Some(MidiMessage::PitchBend {
                channel,
                value: bend.as_int(),
            }),
            MidlyMessage::Aftertouch { key, vel } => Some(MidiMessage::Aftertouch {
                key: key.as_int(),
                channel,
                pressure: vel.as_int(),
            }),
            _ => None,
        }
    }

    fn convert_to_midly_message(msg: &MidiMessage) -> Option<(u8, MidlyMessage)> {
        match msg {
            MidiMessage::NoteOn {
                channel,
                key,
                velocity,
            } => Some((
                *channel,
                MidlyMessage::NoteOn {
                    key: (*key).into(),
                    vel: (*velocity).into(),
                },
            )),
            MidiMessage::NoteOff {
                channel,
                key,
                velocity,
            } => Some((
                *channel,
                MidlyMessage::NoteOff {
                    key: (*key).into(),
                    vel: (*velocity).into(),
                },
            )),
            MidiMessage::ControlChange {
                channel,
                controller,
                value,
            } => Some((
                *channel,
                MidlyMessage::Controller {
                    controller: (*controller).into(),
                    value: (*value).into(),
                },
            )),
            MidiMessage::ProgramChange { channel, program } => Some((
                *channel,
                MidlyMessage::ProgramChange {
                    program: (*program).into(),
                },
            )),
            MidiMessage::PitchBend { channel, value } => Some((
                *channel,
                MidlyMessage::PitchBend {
                    bend: midly::PitchBend::from_int(*value), // Use from_int instead of into
                },
            )),
            _ => None,
        }
    }
}
