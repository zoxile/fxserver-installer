import { mount } from "svelte";
import "@fontsource/jetbrains-mono/400.css";
import "./app.css";
import App from "./App.svelte";

document.documentElement.classList.add("dark");

const app = mount(App, {
	target: document.getElementById("app")!,
});

export default app;
