import { defineConfig } from "vite";
import path from "path";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

// https://vite.dev/config/
export default defineConfig({
	plugins: [svelte(), tailwindcss()],
	optimizeDeps: {
		force: true,
		exclude: ["@lucide/svelte"],
		include: [
			"@tauri-apps/api/core",
			"@tauri-apps/api/app",
			"@tauri-apps/api/path",
			"@tauri-apps/api/window",
			"@tauri-apps/plugin-dialog",
			"bits-ui",
			"d3-scale",
			"d3-shape",
			"layerchart",
		],
	},
	server: {
		proxy: {
			"/api/jg-artifacts/jsonv2": {
				target: "https://artifacts.jgscripts.com",
				changeOrigin: true,
				rewrite: () => "/jsonv2",
			},
		},
	},
	resolve: {
		alias: {
			$lib: path.resolve("./src/lib"),
		},
	},
});
