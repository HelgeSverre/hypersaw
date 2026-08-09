use super::commands::{synchronize_project_runtime_after_restore, Command, DawCommand};
use super::{DawState, EditorView, Project};

const MAX_HISTORY_ENTRIES: usize = 128;

#[derive(Clone, PartialEq, Eq)]
struct AutomationPointHistoryKey {
    clip_id: String,
    lane_id: String,
    point_id: String,
}

#[derive(Clone)]
struct ProjectSessionSnapshot {
    project: Project,
    selected_track: Option<String>,
    selected_clip: Option<String>,
    current_view: EditorView,
}

impl ProjectSessionSnapshot {
    fn capture(state: &DawState) -> Self {
        Self {
            project: state.project.clone(),
            selected_track: state.selected_track.clone(),
            selected_clip: state.selected_clip.clone(),
            current_view: state.current_view.clone(),
        }
    }

    fn restore(&self, state: &mut DawState) {
        let previous_tracks = state.project.tracks.clone();
        let mut project = self.project.clone();

        // Project snapshots restore editable content, but not document identity
        // or live recording state. Save/Save As owns the former; the runtime
        // coordinator owns the latter.
        project.name = state.project.name.clone();
        project.project_path = state.project.project_path.clone();
        project.project_file_path = state.project.project_file_path.clone();
        for restored_track in &mut project.tracks {
            if let Some(previous_track) = previous_tracks
                .iter()
                .find(|track| track.id == restored_track.id)
            {
                restored_track.is_armed = previous_track.is_armed;
                restored_track.input_monitoring = previous_track.input_monitoring;
            } else {
                restored_track.is_armed = false;
                restored_track.input_monitoring = false;
            }
        }

        state.project = project;
        state.selected_track = self.selected_track.clone();
        state.selected_clip = self.selected_clip.clone();
        state.current_view = self.current_view.clone();
        normalize_session_selection(state);
        synchronize_project_runtime_after_restore(state, &previous_tracks);
    }
}

enum HistoryEntry {
    Command {
        command: DawCommand,
        before_revision: u64,
        after_revision: u64,
    },
    Snapshot {
        command_name: &'static str,
        before: Box<ProjectSessionSnapshot>,
        after: Box<ProjectSessionSnapshot>,
        coalesce_key: Option<AutomationPointHistoryKey>,
        before_revision: u64,
        after_revision: u64,
    },
}

impl HistoryEntry {
    fn name(&self) -> &'static str {
        match self {
            Self::Command { command, .. } => command.name(),
            Self::Snapshot { command_name, .. } => command_name,
        }
    }

    fn before_revision(&self) -> u64 {
        match self {
            Self::Command {
                before_revision, ..
            }
            | Self::Snapshot {
                before_revision, ..
            } => *before_revision,
        }
    }

    fn after_revision(&self) -> u64 {
        match self {
            Self::Command { after_revision, .. } | Self::Snapshot { after_revision, .. } => {
                *after_revision
            }
        }
    }
}

pub struct CommandManager {
    undo_stack: Vec<HistoryEntry>,
    redo_stack: Vec<HistoryEntry>,
    current_revision: u64,
    next_revision: u64,
    saved_revision: Option<u64>,
    manually_dirty: bool,
}

impl Default for CommandManager {
    fn default() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_revision: 0,
            next_revision: 1,
            saved_revision: Some(0),
            manually_dirty: false,
        }
    }
}

impl CommandManager {
    pub fn execute(
        &mut self,
        command: DawCommand,
        state: &mut DawState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let uses_snapshot = command.uses_project_snapshot();
        let is_undoable = command.is_undoable();
        let invalidates_redo = command.invalidates_redo();
        let requires_runtime_sync = command.requires_runtime_sync_after_execute();
        let before = uses_snapshot.then(|| ProjectSessionSnapshot::capture(state));
        let command_name = command.name();
        let coalesce_key = automation_point_history_key(&command);

        command.execute(state)?;

        if uses_snapshot {
            if let Some(before) = before {
                normalize_session_selection(state);
                if requires_runtime_sync {
                    synchronize_project_runtime_after_restore(state, &before.project.tracks);
                }
                let after = ProjectSessionSnapshot::capture(state);
                self.push_history(HistoryEntry::Snapshot {
                    command_name,
                    before: Box::new(before),
                    after: Box::new(after),
                    coalesce_key,
                    before_revision: 0,
                    after_revision: 0,
                });
            }
        } else if is_undoable {
            self.push_history(HistoryEntry::Command {
                command,
                before_revision: 0,
                after_revision: 0,
            });
        } else if invalidates_redo {
            self.redo_stack.clear();
        }

        Ok(())
    }

    fn push_history(&mut self, entry: HistoryEntry) {
        if let HistoryEntry::Snapshot {
            after,
            coalesce_key: Some(coalesce_key),
            ..
        } = &entry
        {
            if let Some(HistoryEntry::Snapshot {
                after: previous_after,
                coalesce_key: Some(previous_key),
                after_revision: previous_after_revision,
                ..
            }) = self.undo_stack.last_mut()
            {
                if previous_key == coalesce_key {
                    let after_revision = self.next_revision;
                    self.next_revision += 1;
                    self.current_revision = after_revision;
                    *previous_after = after.clone();
                    *previous_after_revision = after_revision;
                    self.redo_stack.clear();
                    return;
                }
            }
        }

        let before_revision = self.current_revision;
        let after_revision = self.next_revision;
        self.next_revision += 1;
        self.current_revision = after_revision;

        let entry = match entry {
            HistoryEntry::Command { command, .. } => HistoryEntry::Command {
                command,
                before_revision,
                after_revision,
            },
            HistoryEntry::Snapshot {
                command_name,
                before,
                after,
                coalesce_key,
                ..
            } => HistoryEntry::Snapshot {
                command_name,
                before,
                after,
                coalesce_key,
                before_revision,
                after_revision,
            },
        };

        self.undo_stack.push(entry);
        self.redo_stack.clear();
        if self.undo_stack.len() > MAX_HISTORY_ENTRIES {
            if let Some(evicted) = self.undo_stack.first() {
                if self.saved_revision == Some(evicted.before_revision()) {
                    self.saved_revision = None;
                }
            }
            self.undo_stack.remove(0);
        }
    }

    pub fn undo(&mut self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(entry) = self.undo_stack.pop() {
            let result = match &entry {
                HistoryEntry::Command { command, .. } => command.undo(state),
                HistoryEntry::Snapshot { before, .. } => {
                    before.restore(state);
                    Ok(())
                }
            };
            if let Err(error) = result {
                self.undo_stack.push(entry);
                return Err(error);
            }

            self.current_revision = entry.before_revision();
            state.status.info(format!("Undo: {}", entry.name()));
            self.redo_stack.push(entry);
        } else {
            state.status.info("Nothing to undo".to_string());
        }
        Ok(())
    }

    pub fn redo(&mut self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(entry) = self.redo_stack.pop() {
            let result = match &entry {
                HistoryEntry::Command { command, .. } => command.execute(state),
                HistoryEntry::Snapshot { after, .. } => {
                    after.restore(state);
                    Ok(())
                }
            };
            if let Err(error) = result {
                self.redo_stack.push(entry);
                return Err(error);
            }

            self.current_revision = entry.after_revision();
            state.status.info(format!("Redo: {}", entry.name()));
            self.undo_stack.push(entry);
        } else {
            state.status.info("Nothing to redo".to_string());
        }
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.current_revision = 0;
        self.next_revision = 1;
        self.saved_revision = Some(0);
        self.manually_dirty = false;
    }

    pub fn is_project_dirty(&self) -> bool {
        self.manually_dirty || self.saved_revision != Some(self.current_revision)
    }

    pub fn mark_project_dirty(&mut self) {
        // Some persistence flows still mutate `Project` directly (for example
        // recording commits and asynchronous routing). Their state is not in a
        // command snapshot, so retaining history could later overwrite it.
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.current_revision = 0;
        self.next_revision = 1;
        self.saved_revision = None;
        self.manually_dirty = true;
    }

    pub fn mark_project_saved(&mut self) {
        self.break_snapshot_coalescing();
        self.saved_revision = Some(self.current_revision);
        self.manually_dirty = false;
    }

    fn break_snapshot_coalescing(&mut self) {
        if let Some(HistoryEntry::Snapshot { coalesce_key, .. }) = self.undo_stack.last_mut() {
            *coalesce_key = None;
        }
    }
}

fn automation_point_history_key(command: &DawCommand) -> Option<AutomationPointHistoryKey> {
    match command {
        DawCommand::UpdateAutomationPoint {
            clip_id,
            lane_id,
            point_id,
            ..
        } => Some(AutomationPointHistoryKey {
            clip_id: clip_id.clone(),
            lane_id: lane_id.clone(),
            point_id: point_id.clone(),
        }),
        _ => None,
    }
}

fn normalize_session_selection(state: &mut DawState) {
    let selected_track_is_valid = state.selected_track.as_ref().is_none_or(|track_id| {
        state
            .project
            .tracks
            .iter()
            .any(|track| track.id == *track_id)
    });
    if !selected_track_is_valid {
        state.selected_track = None;
    }

    let selected_clip_is_valid = state.selected_clip.as_ref().is_none_or(|clip_id| {
        state.project.tracks.iter().any(|track| {
            track
                .clips
                .iter()
                .any(|clip| matches!(clip, super::Clip::Midi { id, .. } if id == clip_id))
        })
    });
    if !selected_clip_is_valid {
        state.selected_clip = None;
    }

    let view_is_valid = match &state.current_view {
        EditorView::Arrangement => true,
        EditorView::PianoRoll {
            clip_id, track_id, ..
        }
        | EditorView::SampleEditor {
            clip_id, track_id, ..
        } => state.project.tracks.iter().any(|track| {
            track.id == *track_id
                && track
                    .clips
                    .iter()
                    .any(|clip| matches!(clip, super::Clip::Midi { id, .. } if id == clip_id))
        }),
    };
    if !view_is_valid {
        state.current_view = EditorView::Arrangement;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        AutomationLane, AutomationParameter, Clip, MidiEventStore, Take, Track, TrackType,
    };
    use std::path::PathBuf;

    fn state_without_runtime() -> DawState {
        let mut state = DawState::new();
        state.midi_engine = None;
        state.recording_coordinator = None;
        state
    }

    fn midi_track(id: &str) -> Track {
        Track {
            id: id.to_owned(),
            name: "Track".to_owned(),
            track_type: TrackType::Midi {
                channel: 1,
                device_name: None,
                input_device_name: None,
                input_channel: None,
            },
            clips: Vec::new(),
            is_muted: false,
            is_soloed: false,
            is_armed: false,
            input_monitoring: false,
            color: "#ffffff".to_owned(),
            takes: Vec::new(),
            active_take: None,
        }
    }

    fn midi_clip(id: &str, lanes: Vec<AutomationLane>) -> Clip {
        Clip::Midi {
            id: id.to_owned(),
            start_time: 0.0,
            length: 4.0,
            file_path: PathBuf::new(),
            midi_data: Some(MidiEventStore::new(480)),
            loaded: true,
            automation_lanes: lanes,
        }
    }

    #[test]
    fn delete_track_undo_restores_contents_and_session_selection(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = state_without_runtime();
        let mut track = midi_track("track-1");
        track.clips.push(midi_clip("clip-1", Vec::new()));
        state.selected_track = Some(track.id.clone());
        state.selected_clip = Some("clip-1".to_owned());
        state.project.tracks.push(track);
        let mut manager = CommandManager::default();

        manager.execute(
            DawCommand::DeleteTrack {
                track_id: "track-1".to_owned(),
            },
            &mut state,
        )?;
        assert!(state.project.tracks.is_empty());
        assert!(state.selected_track.is_none());
        assert!(state.selected_clip.is_none());

        manager.undo(&mut state)?;
        assert_eq!(state.project.tracks.len(), 1);
        assert_eq!(state.project.tracks[0].clips.len(), 1);
        assert_eq!(state.selected_track.as_deref(), Some("track-1"));
        assert_eq!(state.selected_clip.as_deref(), Some("clip-1"));

        manager.redo(&mut state)?;
        assert!(state.project.tracks.is_empty());
        Ok(())
    }

    #[test]
    fn clip_take_and_automation_mutations_restore_exact_project_data(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = state_without_runtime();
        let mut lane = AutomationLane::new(AutomationParameter::Volume);
        lane.id = "lane-1".to_owned();
        let mut track = midi_track("track-1");
        track.clips = vec![
            midi_clip("clip-1", vec![lane]),
            midi_clip("clip-2", Vec::new()),
        ];
        track.takes = vec![
            Take {
                id: "take-1".to_owned(),
                track_id: track.id.clone(),
                clip_id: "clip-1".to_owned(),
                name: "First".to_owned(),
                timestamp: 1,
                is_muted: false,
            },
            Take {
                id: "take-2".to_owned(),
                track_id: track.id.clone(),
                clip_id: "clip-2".to_owned(),
                name: "Second".to_owned(),
                timestamp: 2,
                is_muted: false,
            },
        ];
        track.active_take = Some("take-2".to_owned());
        state.selected_clip = Some("clip-2".to_owned());
        state.project.tracks.push(track);
        let mut manager = CommandManager::default();

        manager.execute(
            DawCommand::DeleteTake {
                track_id: "track-1".to_owned(),
                take_id: "take-2".to_owned(),
            },
            &mut state,
        )?;
        assert_eq!(state.project.tracks[0].takes.len(), 1);
        assert_eq!(state.project.tracks[0].clips.len(), 1);
        manager.undo(&mut state)?;
        assert_eq!(state.project.tracks[0].takes.len(), 2);
        assert_eq!(state.project.tracks[0].clips.len(), 2);
        assert_eq!(
            state.project.tracks[0].active_take.as_deref(),
            Some("take-2")
        );
        assert_eq!(state.selected_clip.as_deref(), Some("clip-2"));

        manager.execute(
            DawCommand::DeleteClip {
                track_id: "track-1".to_owned(),
                clip_id: "clip-1".to_owned(),
            },
            &mut state,
        )?;
        assert_eq!(state.project.tracks[0].clips.len(), 1);
        manager.undo(&mut state)?;
        assert_eq!(state.project.tracks[0].clips.len(), 2);

        manager.execute(
            DawCommand::AddAutomationPoint {
                clip_id: "clip-1".to_owned(),
                lane_id: "lane-1".to_owned(),
                time: 1.0,
                value: 0.25,
            },
            &mut state,
        )?;
        assert_eq!(automation_point_count(&state), 1);
        manager.undo(&mut state)?;
        assert_eq!(automation_point_count(&state), 0);
        manager.redo(&mut state)?;
        assert_eq!(automation_point_count(&state), 1);
        Ok(())
    }

    #[test]
    fn routing_and_bpm_snapshots_round_trip_and_respect_savepoints(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = state_without_runtime();
        state.project.tracks.push(midi_track("track-1"));
        let mut manager = CommandManager::default();

        manager.execute(
            DawCommand::SetTrackMidiInputPort {
                track_id: "track-1".to_owned(),
                input_port: Some("Keyboard".to_owned()),
            },
            &mut state,
        )?;
        manager.execute(
            DawCommand::SetTrackMidiInputChannel {
                track_id: "track-1".to_owned(),
                channel: Some(7),
            },
            &mut state,
        )?;
        assert_eq!(input_routing(&state), (Some("Keyboard"), Some(7)));
        manager.undo(&mut state)?;
        assert_eq!(input_routing(&state), (Some("Keyboard"), None));
        manager.redo(&mut state)?;
        assert_eq!(input_routing(&state), (Some("Keyboard"), Some(7)));

        manager.execute(DawCommand::SetBpm { bpm: 140.0 }, &mut state)?;
        manager.mark_project_saved();
        assert!(!manager.is_project_dirty());
        manager.execute(DawCommand::SetBpm { bpm: 160.0 }, &mut state)?;
        assert!(manager.is_project_dirty());
        manager.undo(&mut state)?;
        assert_eq!(state.project.bpm, 140.0);
        assert!(!manager.is_project_dirty());
        manager.undo(&mut state)?;
        assert_eq!(state.project.bpm, 120.0);
        assert_eq!(input_routing(&state), (Some("Keyboard"), Some(7)));
        assert!(manager.is_project_dirty());
        manager.redo(&mut state)?;
        assert_eq!(state.project.bpm, 140.0);
        assert!(!manager.is_project_dirty());
        Ok(())
    }

    #[test]
    fn automation_point_drag_updates_coalesce_without_losing_the_original_value(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = state_without_runtime();
        let mut lane = AutomationLane::new(AutomationParameter::Volume);
        lane.id = "lane-1".to_owned();
        let point_id = lane.add_point(0.0, 0.1);
        let mut track = midi_track("track-1");
        track.clips.push(midi_clip("clip-1", vec![lane]));
        state.project.tracks.push(track);
        let mut manager = CommandManager::default();

        for time in [1.0, 2.0, 3.0] {
            manager.execute(
                DawCommand::UpdateAutomationPoint {
                    clip_id: "clip-1".to_owned(),
                    lane_id: "lane-1".to_owned(),
                    point_id: point_id.clone(),
                    time: Some(time),
                    value: None,
                },
                &mut state,
            )?;
        }

        assert_eq!(manager.undo_stack.len(), 1);
        manager.undo(&mut state)?;
        assert_eq!(automation_point_time(&state), 0.0);
        manager.redo(&mut state)?;
        assert_eq!(automation_point_time(&state), 3.0);
        Ok(())
    }

    #[test]
    fn saving_between_automation_updates_preserves_the_savepoint_in_history(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = state_without_runtime();
        let mut lane = AutomationLane::new(AutomationParameter::Volume);
        lane.id = "lane-1".to_owned();
        let point_id = lane.add_point(0.0, 0.1);
        let mut track = midi_track("track-1");
        track.clips.push(midi_clip("clip-1", vec![lane]));
        state.project.tracks.push(track);
        let mut manager = CommandManager::default();

        manager.execute(
            DawCommand::UpdateAutomationPoint {
                clip_id: "clip-1".to_owned(),
                lane_id: "lane-1".to_owned(),
                point_id: point_id.clone(),
                time: Some(1.0),
                value: None,
            },
            &mut state,
        )?;
        manager.mark_project_saved();
        manager.execute(
            DawCommand::UpdateAutomationPoint {
                clip_id: "clip-1".to_owned(),
                lane_id: "lane-1".to_owned(),
                point_id,
                time: Some(2.0),
                value: None,
            },
            &mut state,
        )?;

        assert_eq!(manager.undo_stack.len(), 2);
        manager.undo(&mut state)?;
        assert_eq!(automation_point_time(&state), 1.0);
        assert!(!manager.is_project_dirty());
        manager.redo(&mut state)?;
        assert_eq!(automation_point_time(&state), 2.0);
        assert!(manager.is_project_dirty());
        Ok(())
    }

    #[test]
    fn snapshot_restore_preserves_live_track_flags_and_disarms_resurrected_tracks(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = state_without_runtime();
        state.project.tracks.push(midi_track("track-1"));
        let mut manager = CommandManager::default();

        manager.execute(
            DawCommand::RenameTrack {
                track_id: "track-1".to_owned(),
                new_name: "Renamed".to_owned(),
            },
            &mut state,
        )?;
        state.project.tracks[0].is_armed = true;
        state.project.tracks[0].input_monitoring = true;
        manager.undo(&mut state)?;
        assert_eq!(state.project.tracks[0].name, "Track");
        assert!(state.project.tracks[0].is_armed);
        assert!(state.project.tracks[0].input_monitoring);

        manager.execute(
            DawCommand::DeleteTrack {
                track_id: "track-1".to_owned(),
            },
            &mut state,
        )?;
        manager.undo(&mut state)?;
        assert!(!state.project.tracks[0].is_armed);
        assert!(!state.project.tracks[0].input_monitoring);
        Ok(())
    }

    #[test]
    fn snapshot_restore_keeps_the_current_document_identity(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = state_without_runtime();
        let mut manager = CommandManager::default();

        manager.execute(DawCommand::SetBpm { bpm: 140.0 }, &mut state)?;
        state.project.name = "Saved Project".to_owned();
        state.project.project_path = Some(PathBuf::from("/tmp/saved-project"));
        state.project.project_file_path =
            Some(PathBuf::from("/tmp/saved-project/saved-project.supersaw"));

        manager.undo(&mut state)?;
        assert_eq!(state.project.bpm, 120.0);
        assert_eq!(state.project.name, "Saved Project");
        assert_eq!(
            state.project.project_path.as_deref(),
            Some(std::path::Path::new("/tmp/saved-project"))
        );
        assert_eq!(
            state.project.project_file_path.as_deref(),
            Some(std::path::Path::new(
                "/tmp/saved-project/saved-project.supersaw"
            ))
        );
        manager.redo(&mut state)?;
        assert_eq!(state.project.bpm, 140.0);
        assert_eq!(state.project.name, "Saved Project");
        Ok(())
    }

    #[test]
    fn evicted_history_cannot_return_to_an_unreachable_savepoint(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = state_without_runtime();
        let mut manager = CommandManager::default();

        for bpm in 121..=(120 + MAX_HISTORY_ENTRIES as i32 + 1) {
            manager.execute(
                DawCommand::SetBpm {
                    bpm: f64::from(bpm),
                },
                &mut state,
            )?;
        }

        assert_eq!(manager.undo_stack.len(), MAX_HISTORY_ENTRIES);
        while manager.can_undo() {
            manager.undo(&mut state)?;
        }
        assert!(manager.is_project_dirty());
        assert_eq!(state.project.bpm, 121.0);
        Ok(())
    }

    #[test]
    fn transient_recording_controls_do_not_enter_project_history(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = state_without_runtime();
        state.project.tracks.push(midi_track("track-1"));
        let mut manager = CommandManager::default();

        manager.execute(
            DawCommand::ArmTrack {
                track_id: "track-1".to_owned(),
            },
            &mut state,
        )?;

        assert!(state.project.tracks[0].is_armed);
        assert!(!manager.can_undo());
        assert!(!manager.is_project_dirty());
        Ok(())
    }

    #[test]
    fn external_project_mutations_create_a_history_barrier(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = state_without_runtime();
        state.project.tracks.push(midi_track("track-1"));
        let mut manager = CommandManager::default();

        manager.execute(
            DawCommand::RenameTrack {
                track_id: "track-1".to_owned(),
                new_name: "History rename".to_owned(),
            },
            &mut state,
        )?;
        state.project.tracks.push(midi_track("external-track"));
        manager.mark_project_dirty();

        assert!(!manager.can_undo());
        assert!(!manager.can_redo());
        assert!(manager.is_project_dirty());
        manager.undo(&mut state)?;
        manager.redo(&mut state)?;
        assert_eq!(state.project.tracks.len(), 2);
        assert_eq!(state.project.tracks[0].name, "History rename");
        assert_eq!(state.project.tracks[1].id, "external-track");
        Ok(())
    }

    fn automation_point_count(state: &DawState) -> usize {
        let Clip::Midi {
            automation_lanes, ..
        } = &state.project.tracks[0].clips[0];
        automation_lanes[0].points.len()
    }

    fn automation_point_time(state: &DawState) -> f64 {
        let Clip::Midi {
            automation_lanes, ..
        } = &state.project.tracks[0].clips[0];
        automation_lanes[0].points[0].time
    }

    fn input_routing(state: &DawState) -> (Option<&str>, Option<u8>) {
        let TrackType::Midi {
            input_device_name,
            input_channel,
            ..
        } = &state.project.tracks[0].track_type;
        (input_device_name.as_deref(), *input_channel)
    }
}
