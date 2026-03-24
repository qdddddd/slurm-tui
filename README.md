# slurm-tui

A terminal dashboard for monitoring and interacting with a Slurm HPC cluster,
built with Rust and [Ratatui](https://github.com/ratatui/ratatui).

### Dark mode
![dark mode](screenshots/dark.png)

### Light mode
![light mode](screenshots/light.png)

### Submit job
![submit job](screenshots/submit.png)

### Cancel job
![cancel job](screenshots/cancel.png)

## Features

- Live-updating job queue and cluster/node status
- Running job details with resource usage and log file paths
- Log tail cleanup for carriage-return progress bars and ANSI-colored output
- Historical log access for your recent jobs through a selector opened with `l`
- Keyboard-driven actions:
  - `s` submit a job (`sbatch`)
  - `c` cancel a job (`scancel`)
  - `d` change working directory
  - `l` select one of your previous 10 top-level jobs and open its log in full-screen `less`
  - `q` quit

## Requirements

- Rust toolchain (1.85+)
- Access to Slurm CLI tools (`squeue`, `sinfo`, `scontrol`, `sacct`, `sbatch`, `scancel`)
- `less` available in `PATH` for historical log viewing

## Build

```bash
cargo build --release
```

## Usage

```bash
./target/release/slurm-tui
```

### Options

```text
--dark              Use dark background theme (default: light)
--login-node HOST   SSH host for resolving UIDs to usernames
-n SECS             Refresh interval in seconds (supports decimals, default: 1)
```

### Examples

```bash
./target/release/slurm-tui
./target/release/slurm-tui --dark
./target/release/slurm-tui --login-node 112
./target/release/slurm-tui -n 0.5
```

## Historical logs

Press `l` to open a selector showing your previous 10 top-level jobs from `sacct`.

- `Up` / `Down` moves the selection
- `Enter` resolves the selected job's stdout path and opens it in full-screen `less`
- `Esc` closes the selector

For completed jobs, stdout resolution falls back to `sacct` and expands common Slurm path templates such as `%j`, `%A`, and `%x`.

