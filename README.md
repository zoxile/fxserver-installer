# FXServer Installer, Built with Svelte + TS + Tauri + Vite + Redbull
# Styling done with Tailwind CSS + Shadcn

I've seen a lot of people struggle with installing servers. For some it might be very simple, but for new server owners this might be uncharted territory, this is where this project comes in!
This app does streamline things and make it a lot faster, meaning that it's useful even for users that knows how to setup servers as it can be a drag to install everything one by one on new machines!

## What does it do?
Glad you asked!
1. Install [MariaDB](https://mariadb.org/)
2. Setup MariaDB (Credentials and so on)
3. Install healthy artifact (Fetched from [Artifacts JG Scripts](https://artifacts.jgscripts.com/). Thank you [JG](https://github.com/jgscripts)!)
4. Give you instructions on what to do while it's installing.
5. Extract installed .zip file.
6. Run the FXServer.exe file inside the installed artifact to start the server installation process.
7. Provide you with management features of MariaDB service but also extra tips and tricks.
8. Provide you with extra utilities that you may need.

## Why do such a thing?

Honestly, how effective this project will be is a big question mark. It might be a very stupid project to do or a brilliant project to do. Someone else might have done this or not, didn't really research.
I was just wanting to create an application using (Svelte + TS + Tauri). This was my first project using Tauri so it was a good chance to learn what I can do with it.

## What's planned ahead?

- Making it functional for Linux
- Config editor. Drop in your config file or open it to change values.
- Profiler Display. Drop in your profiler's .json file to view the profiler values and optimize your server.

So far this is what I've got. This makes it branch away from being just a "FXServer Installer", although I don't think that there's anything wrong with that. Also I might do these things and I might not. It really depends, so it's not really a guarantee.

## Legal

This project is dedicated to the public domain under CC0 1.0.

You can use, modify, distribute, and sell this software without restriction.  
No attribution is required.

See the [LICENSE](./LICENSE) file for details.