import { defineConfig } from "vite";
import path from "path";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

// https://vite.dev/config/
export default defineConfig({
	plugins: [svelte(), tailwindcss()],
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
