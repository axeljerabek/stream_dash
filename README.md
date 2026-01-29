🍓 stream_dash (v23.0-rs)
The evolution of [stream_top](https://github.com/axeljerabek/stream_top). A high-performance, Rust-based diagnostic dashboard for Raspberry Pi power users. While the original Bash version focused on raw numbers, stream_dash introduces Real-Time History Graphs to visualize hardware trends over time.

🔍 Why stream_dash?
Standard monitoring tools (like htop or btop) often miss what matters for video streaming: DMA/CMA buffer states, VPU voltages, and H.264 clock speeds.

stream_dash is written in Rust for minimal CPU footprint and provides:

Visual History: 42-point sparkline graphs for CPU and Temperature.

Stacked Memory Graphs: Visualizing Active vs. Reserved vs. Total memory for both RAM and CMA.

Deep Hardware Insight: Real-time throttling status and DMA-buffer breakdown.

Systemd Integration: Integrated, scrollable logs for your stream.service.

📸 Visualization Logic
Triple-Stacked Bars (RAM & CMA)
We use a unique layered visualization to show memory health at a glance:

█ Active (Bright): Data actually in use (App code or live video frames).

▓ Reserved/Cached (Dim): System-managed buffers (Linux Cache or CMA pre-allocations).

░ Total Capacity (Dark): Your hardware ceiling.

🛠 Installation
1. Requirements
Ensure you have the Rust toolchain installed:

Bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
2. Build & Run
Bash
git clone https://github.com/axeljerabek/stream_dash.git
cd stream_dash
cargo build --release
sudo ./target/release/stream_dash
Note: sudo is required to read the kernel's DMA debug interface.

🎮 Interactive Controls
+ / - : Adjust Refresh Speed (100ms to 5s).

, / . : Adjust Log Depth (Show more/fewer lines).

q : Exit.

🔗 History
This project is the spiritual successor to stream_top. While stream_top remains a great lightweight Bash alternative, stream_dash is recommended for users who need to see "the story" behind the numbers through historical graphing.
