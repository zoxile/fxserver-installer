# FXServer Installer (Svelte + TS + Tauri + Vite + Redbull)
> A simple tool to install and set up an FXServer without the usual hassle.

## Tech Stack
- Svelte (TypeScript) — UI
- Tauri — backend/runtime
- Tailwind CSS v4 + shadcn-svelte — styling
- Vite — dev/build tooling
- npm — package management

## Why?

I've seen a lot of people struggle with installing servers. For some, it’s simple, but for new server owners, it can feel like uncharted territory.

That’s where this project comes in.

This app streamlines the process and makes everything a lot faster. Even if you already know how to set things up, it saves you from doing the same repetitive steps every time you’re on a new machine.

## What does it do?

Glad you asked:

1. Installs [MariaDB](https://mariadb.org/)
2. Sets up MariaDB (credentials and basic configuration)
3. Downloads a healthy artifact (from [Artifacts @ JG Scripts](https://artifacts.jgscripts.com/). Big thanks to [JG](https://github.com/jgscripts)!)
4. Guides you through what’s happening during installation
5. Extracts the downloaded `.zip`
6. Runs the FXServer executable to kick off setup
7. Provides basic MariaDB management tools + helpful tips
8. Includes extra utilities you might need

## Why did I make this?

Honestly, I didn’t overthink it too much.

I wanted to build something with Svelte + Tauri and learn what I could do with it. This just felt like a useful (and practical) idea to explore.

My main goal isn’t to get as many users as possible. I just want to learn how to build applications using Tauri and properly use GitHub without turning my commit history into chaos (still working on that).

If it ends up helping people, great. If not, I still learned a lot building it.

## What's planned?

- Linux support
- Config editor (drop in or edit your config files directly)
- Profiler viewer (load `.json` files to analyze performance)

No strict promises here, it's just ideas I’d like to explore next (if all goes well).

## Legal

This project is dedicated to the public domain under CC0 1.0.

You can use, modify, distribute, and sell this software without restriction.  
No attribution required.

See the [LICENSE](./LICENSE) file for details.