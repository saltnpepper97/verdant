# Verdant

**Verdant** is a minimal, modern init system for Linux — designed for resilience, clarity, and a more sustainable future. Built in Rust, it emphasizes simplicity, transparency, and clean service supervision without legacy cruft.

> 🌱 *Solarpunk-inspired. Cybersecurity-conscious. Compatible with musl and glibc.*

## Features

- 🔧 **Clean Boot Flow** — Starts with `init` as PID 1, then hands off to `verdantd` for service supervision.
- 🔁 **Modular Service Management** — Declarative `.vs` files with support for dependencies and reloading.
- ⚡ **Lightweight and Fast** — No D-Bus. No shell scripts. No surprises. Just Rust.
- 📦 **Musl-Compatible** — Fully functional on both musl and glibc systems.
- 🔐 **Security-Oriented** — Small trusted computing base, with future support planned for seccomp, sandboxing, and privilege separation.

## Components

- **`init`** – The PID 1 binary. Mounts `/proc`, `/dev`, `/sys`, `/tmp`, sets the hostname, and loads kernel modules.
- **`verdantd`** – The service supervisor. Manages long-running daemons defined via `.vs` files.
- **`vctl`** – Command-line interface for starting, stopping, reloading, and inspecting services.

## Status

🧪 **Experimental** — Verdant is under active development and not yet production-ready. Contributions, feedback, and testing are very welcome!