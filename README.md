# Hypersaw - DAW built in Rust with egui

## Install

```shell
git clone git@github.com:HelgeSverre/hypersaw.git
cd hypersaw

# Clone the VST3 SDK into the root of the project
git clone --recursive https://github.com/steinbergmedia/vst3sdk.git

# Install dependencies
cargo install --path .

# Build the project
cargo build

# Run the project
cargo run
```