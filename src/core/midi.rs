use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub type NoteID = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MidiEvent {
    pub tick: u32,
    pub message: MidiMessageWrapper,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MidiMessageWrapper {
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
    Controller {
        channel: u8,
        controller: u8,
        value: u8,
    },
    PitchBend {
        channel: u8,
        value: i16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MidiNote {
    pub id: String,
    pub start_time: f64,
    pub start_tick: u32,
    pub duration: f64,
    pub duration_ticks: u32,
    pub pitch: u8,
    pub velocity: u8,
    pub channel: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MidiClipData {
    pub notes: Vec<MidiNote>,
    pub events: Vec<MidiEvent>,
    pub events_by_tick: HashMap<u32, Vec<MidiEvent>>,
    pub length: f64,
    pub length_ticks: u32,
    pub ppq: u32,
    pub tempo_map: Vec<TempoChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TempoChange {
    pub tick: u32,
    pub tempo: u32,
}

impl MidiClipData {
    pub fn get_events_at_tick(&self, tick: u32) -> &[MidiEvent] {
        self.events_by_tick.get(&tick).map_or(&[], |v| v.as_slice())
    }

    pub fn tick_to_time(&self, tick: u32) -> f64 {
        let tempo_change = self
            .tempo_map
            .iter()
            .rev()
            .find(|tc| tc.tick <= tick)
            .unwrap_or(&self.tempo_map[0]);

        let tick_delta = tick - tempo_change.tick;
        let seconds_per_tick = tempo_change.tempo as f64 / (self.ppq as f64 * 1_000_000.0);

        tick_delta as f64 * seconds_per_tick
    }

    pub fn time_to_tick(&self, time: f64) -> u32 {
        let default_tempo = self.tempo_map[0].tempo;
        let ticks_per_second = (self.ppq as f64 * 1_000_000.0) / default_tempo as f64;
        (time * ticks_per_second) as u32
    }

    pub fn load_from_file(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        use midly::{Smf, TrackEventKind};
        use std::fs::File;
        use std::io::Read;

        let mut file = File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        let smf = Smf::parse(&buffer)?;
        let ppq = 480; // TODO: get from smf

        let mut notes = Vec::new();
        let mut events = Vec::new();
        let mut events_by_tick = HashMap::new();
        let mut pending_notes = HashMap::<String, (u32, i32)>::new();
        let mut current_time_ticks = 0;
        let mut max_time_ticks = 0;
        let mut tempo_map = vec![TempoChange {
            tick: 0,
            tempo: 500_000,
        }];

        for track in smf.tracks {
            current_time_ticks = 0;

            for event in track {
                current_time_ticks += event.delta.as_int();
                max_time_ticks = max_time_ticks.max(current_time_ticks);

                if let Some(midi_event) = convert_midi_event(current_time_ticks, event.kind) {
                    events.push(midi_event.clone());
                    events_by_tick
                        .entry(current_time_ticks)
                        .or_insert_with(Vec::new)
                        .push(midi_event);
                }

                match event.kind {
                    TrackEventKind::Midi { message, channel } => match message {
                        midly::MidiMessage::NoteOn { key, vel } => {
                            if vel.as_int() > 0 {
                                let note_id = format!("{}:{}", channel.as_int(), key.as_int());
                                pending_notes
                                    .insert(note_id, (current_time_ticks, vel.as_int().into()));
                            } else {
                                handle_note_off(
                                    &mut notes,
                                    &mut pending_notes,
                                    channel.as_int(),
                                    key.as_int(),
                                    current_time_ticks,
                                    &tempo_map,
                                    ppq,
                                );
                            }
                        }
                        midly::MidiMessage::NoteOff { key, .. } => {
                            handle_note_off(
                                &mut notes,
                                &mut pending_notes,
                                channel.as_int(),
                                key.as_int(),
                                current_time_ticks,
                                &tempo_map,
                                ppq,
                            );
                        }
                        _ => {}
                    },
                    TrackEventKind::Meta(midly::MetaMessage::Tempo(tempo)) => {
                        tempo_map.push(TempoChange {
                            tick: current_time_ticks,
                            tempo: tempo.as_int(),
                        });
                    }
                    _ => {}
                }
            }
        }

        let length = calculate_length(max_time_ticks, &tempo_map, ppq);

        Ok(MidiClipData {
            notes,
            events,
            events_by_tick,
            length,
            length_ticks: max_time_ticks,
            ppq,
            tempo_map,
        })
    }
}

use midly::TrackEventKind;

pub fn convert_midi_event(tick: u32, event: TrackEventKind) -> Option<MidiEvent> {
    match event {
        TrackEventKind::Midi { message, channel } => {
            let msg = match message {
                midly::MidiMessage::NoteOn { key, vel } => MidiMessageWrapper::NoteOn {
                    channel: channel.as_int(),
                    key: key.as_int(),
                    velocity: vel.as_int(),
                },
                midly::MidiMessage::NoteOff { key, vel } => MidiMessageWrapper::NoteOff {
                    channel: channel.as_int(),
                    key: key.as_int(),
                    velocity: vel.as_int(),
                },
                _ => return None,
            };
            Some(MidiEvent { tick, message: msg })
        }
        _ => None,
    }
}

pub fn handle_note_off(
    notes: &mut Vec<MidiNote>,
    pending_notes: &mut HashMap<String, (u32, i32)>,
    channel: u8,
    key: u8,
    current_ticks: u32,
    tempo_map: &[TempoChange],
    ppq: u32,
) {
    let note_id = format!("{}:{}", channel, key);
    if let Some((start_tick, velocity)) = pending_notes.remove(&note_id) {
        let start_time = calculate_time(start_tick, tempo_map, ppq);
        let end_time = calculate_time(current_ticks, tempo_map, ppq);

        notes.push(MidiNote {
            id: uuid::Uuid::new_v4().to_string(),
            start_time,
            start_tick,
            duration: end_time - start_time,
            duration_ticks: current_ticks - start_tick,
            pitch: key,
            velocity: velocity as u8,
            channel,
        });
    }
}

pub fn calculate_time(tick: u32, tempo_map: &[TempoChange], ppq: u32) -> f64 {
    let mut time = 0.0;
    let mut current_tick = 0;

    for window in tempo_map.windows(2) {
        let current_tempo = window[0].tempo;
        let next_tempo_tick = window[1].tick;

        if tick < next_tempo_tick {
            let tick_delta = tick - current_tick;
            time += (tick_delta as f64 * current_tempo as f64) / (ppq as f64 * 1_000_000.0);
            break;
        } else {
            let tick_delta = next_tempo_tick - current_tick;
            time += (tick_delta as f64 * current_tempo as f64) / (ppq as f64 * 1_000_000.0);
            current_tick = next_tempo_tick;
        }
    }

    if tick > current_tick {
        let last_tempo = tempo_map.last().unwrap().tempo;
        let tick_delta = tick - current_tick;
        time += (tick_delta as f64 * last_tempo as f64) / (ppq as f64 * 1_000_000.0);
    }

    time
}

pub fn calculate_length(max_ticks: u32, tempo_map: &[TempoChange], ppq: u32) -> f64 {
    calculate_time(max_ticks, tempo_map, ppq)
}
