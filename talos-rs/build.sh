#!/bin/bash

# Script to install Rust and build the Talos project
# Created as part of MVP implementation

echo "Checking for Rust installation..."

if ! command -v rustc &> /dev/null; then
    echo "Rust is not installed. Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source ~/.cargo/env
else
    echo "Rust is already installed."
    rustc --version
fi

# Navigate to the project directory
cd talos-rs

echo "Building the project..."
cargo build

if [ $? -eq 0 ]; then
    echo "Build successful!"
    echo "To run the server, use: cargo run -p talos"
    echo "Make sure to set required environment variables:"
    echo "export ANTHROPIC_API_KEY=your_api_key_here"
    echo "or update talos.yaml with your preferred LLM provider"
else
    echo "Build failed!"
    exit 1
fi