# DAWproject Support in HyperSaw

This document describes the DAWproject import/export functionality added to HyperSaw.

## Overview

DAWproject is an open exchange format for user data between Digital Audio Workstations (DAWs). HyperSaw now supports both importing and exporting projects in the DAWproject format, enabling interoperability with other DAWs that support this standard.

## Supported Features

### Export
- Project metadata (name, BPM, time signature)
- Track structure with proper channel routing
- MIDI tracks with note data
- Audio tracks (structure only, media files are copied)
- Track properties (mute, solo, color, volume, pan)
- Master track configuration

### Import
- Basic project properties (BPM, time signature)
- Track structure recreation
- Track properties restoration

## Usage

### Exporting to DAWproject

1. In HyperSaw, go to **File > Export DAWproject...**
2. Choose a location and filename for your .dawproject file
3. Click Save

The exported file will contain:
- `project.xml` - Main project structure and data
- `metadata.xml` - Project metadata  
- `audio/` - Audio files (if any)
- `midi/` - MIDI files (if any)

### Importing from DAWproject

1. In HyperSaw, go to **File > Import DAWproject...**
2. Select a .dawproject file
3. Click Open

The project will be loaded with all supported elements.

## Technical Details

### File Format
- Container: ZIP archive with .dawproject extension
- Primary content: XML files conforming to DAWproject 1.0 specification
- Media files: Stored in subdirectories within the archive

### Mapping between HyperSaw and DAWproject

| HyperSaw Element | DAWproject Element | Notes |
|------------------|-------------------|-------|
| Project | Project | BPM, time signature, name |
| Track | Track + Channel | Type, name, color, routing |
| MIDI Clip | Clip with Notes | Note data, timing |
| Audio Clip | Clip with Audio | File references |
| Note | Note | Pitch, velocity, timing |

### Limitations
- Import functionality is basic and may not preserve all details
- Audio file handling requires files to be accessible
- Some advanced features may not transfer between DAWs

## Implementation

The DAWproject support is implemented in `src/core/dawproject.rs` with:
- XML serialization/deserialization using `quick-xml`
- ZIP archive handling using `zip` crate
- Proper mapping between internal and DAWproject formats
- UI integration in the File menu

## Testing

Basic functionality can be tested using the included test suite:

```bash
cargo test dawproject
```

This will run tests that verify export/import round-trip functionality.

## Compatibility

This implementation follows the DAWproject 1.0 specification and should be compatible with other DAWs that support this format, including:
- Bitwig Studio
- PreSonus Studio One  
- Steinberg Cubase
- Cockos Reaper (via converter)

## Future Enhancements

Potential improvements could include:
- More complete import functionality
- Support for automation data
- Plugin state preservation
- Enhanced audio file handling
- Validation against DAWproject schemas