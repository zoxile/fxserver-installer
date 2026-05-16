<script lang="ts">
	import BracesIcon from "@lucide/svelte/icons/braces";
	import ClipboardIcon from "@lucide/svelte/icons/clipboard";
	import FileJsonIcon from "@lucide/svelte/icons/file-json";
	import Minimize2Icon from "@lucide/svelte/icons/minimize-2";
	import SparklesIcon from "@lucide/svelte/icons/sparkles";
	import WandSparklesIcon from "@lucide/svelte/icons/wand-sparkles";
	import UploadIcon from "@lucide/svelte/icons/upload";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { log } from "$lib/core/logger.svelte";
	import JsonNotice from "./JsonNotice.svelte";
	import { formatJson, getJsonErrorMessage, minifyJson, repairJson, tryParseJson } from "./jsonRepair";

	let input = $state('{\n  "resource": "fxserver",\n  "enabled": true,\n  "ports": [30120]\n}');
	let output = $state("");
	let fixedOutput = $state("");
	let notice = $state<{ type: "success" | "error" | "repair"; title: string; description: string } | null>(null);
	const inputPlaceholder = '{"name":"fxserver","enabled":true}';

	let fileInput: HTMLInputElement;

	async function uploadJsonFile(event: Event) {
		const target = event.target as HTMLInputElement;
		const file = target.files?.[0];

		if (!file) return;

		if (!file.name.endsWith(".json")) {
			log("JSON upload rejected because the file extension was invalid.", { level: "warn", scope: "json.formatter", detail: file.name });
			notice = {
				type: "error",
				title: "Invalid file type",
				description: "Please upload a .json file.",
			};
			return;
		}

		try {
			input = await file.text();
			output = "";
			fixedOutput = "";
			log("JSON file uploaded into the formatter.", { level: "success", scope: "json.formatter", detail: `${file.name} (${input.length} characters)` });

			notice = {
				type: "success",
				title: "JSON file uploaded",
				description: `${file.name} has been loaded into the editor.`,
			};
		} catch {
			log("JSON file upload failed.", { level: "error", scope: "json.formatter", detail: file.name });
			notice = {
				type: "error",
				title: "Could not read file",
				description: "The selected file could not be loaded.",
			};
		}

		target.value = "";
	}

	function formatInput() {
		fixedOutput = "";

		try {
			const value = tryParseJson(input);
			output = formatJson(value);
			log("JSON formatted successfully.", { level: "success", scope: "json.formatter", detail: `${output.length} characters` });
			notice = {
				type: "success",
				title: "JSON is valid",
				description: "Formatted with two-space indentation.",
			};
		} catch (error) {
			const repair = repairJson(input);
			output = "";

			if (repair) {
				fixedOutput = repair.json;
				log("JSON formatter found a fixable syntax issue.", { level: "warn", scope: "json.formatter", detail: repair.changes.join(", ") });
				notice = {
					type: "repair",
					title: "Fixable JSON issue found",
					description: `${getJsonErrorMessage(input, error)} Suggested repair: ${repair.changes.join(", ")}.`,
				};
			} else {
				log("JSON formatting failed with an unrecoverable syntax issue.", { level: "error", scope: "json.formatter", detail: getJsonErrorMessage(input, error) });
				notice = {
					type: "error",
					title: "Invalid JSON",
					description: getJsonErrorMessage(input, error),
				};
			}
		}
	}

	function minifyInput() {
		fixedOutput = "";

		try {
			output = minifyJson(tryParseJson(input));
			log("JSON minified successfully.", { level: "success", scope: "json.formatter", detail: `${output.length} characters` });
			notice = {
				type: "success",
				title: "JSON minified",
				description: "Whitespace was removed from a valid JSON document.",
			};
		} catch (error) {
			output = "";
			log("JSON minify failed.", { level: "error", scope: "json.formatter", detail: getJsonErrorMessage(input, error) });
			notice = {
				type: "error",
				title: "Cannot minify invalid JSON",
				description: getJsonErrorMessage(input, error),
			};
		}
	}

	function useFixedJson() {
		input = fixedOutput;
		output = fixedOutput;
		fixedOutput = "";
		log("Suggested JSON repair was applied.", { level: "success", scope: "json.formatter" });
		notice = {
			type: "success",
			title: "Applied fixed JSON",
			description: "The repaired JSON has been moved into the editor.",
		};
	}

	async function copyText(value: string, label: string) {
		if (!value) return;
		await navigator.clipboard.writeText(value);
		log(`${label} copied to clipboard.`, { level: "debug", scope: "json.formatter" });
		notice = {
			type: "success",
			title: `${label} copied`,
			description: "The JSON text is now on your clipboard.",
		};
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Tools</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">JSON Formatter</h1>
			<p class="mt-2 max-w-2xl text-sm text-muted-foreground">Format, minify, validate, and repair common JSON syntax mistakes before using resource data.</p>
		</div>
		<div class="inline-flex items-center gap-2 rounded-sm border border-border bg-card px-3 py-2 text-xs text-muted-foreground">
			<FileJsonIcon class="size-3.5" />
			Strict JSON parser
		</div>
	</div>

	{#if notice}
		<JsonNotice {...notice} />
	{/if}

	<div class="grid gap-4 xl:grid-cols-12">
		<Card.Root class="rounded-md border-border bg-card shadow-sm xl:col-span-7">
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<BracesIcon class="size-5" />
					</div>
					<div>
						<Card.Title>Editor</Card.Title>
						<Card.Description>Paste JSON or common JSON-like config snippets here.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="flex flex-wrap gap-2">
					<input bind:this={fileInput} type="file" accept=".json,application/json" class="hidden" onchange={uploadJsonFile} />

					<Button variant="outline" onclick={() => fileInput.click()} title="Upload JSON file">
						<UploadIcon />
						Upload JSON
					</Button>
					<Button onclick={formatInput} title="Format and validate JSON">
						<SparklesIcon />
						Format
					</Button>
					<Button variant="outline" onclick={minifyInput} title="Minify valid JSON">
						<Minimize2Icon />
						Minify
					</Button>
					<Button variant="outline" onclick={() => copyText(input, "Input")} title="Copy editor JSON">
						<ClipboardIcon />
						Copy Input
					</Button>
				</div>
				<textarea
					bind:value={input}
					spellcheck="false"
					placeholder={inputPlaceholder}
					title="JSON input to validate, format, minify, or repair."
					class="min-h-48 w-full resize-y rounded-sm border border-input bg-background px-3 py-3 font-mono text-sm leading-6 text-foreground outline-none transition-colors placeholder:text-muted-foreground focus:border-ring focus:ring-2 focus:ring-ring/30"
				></textarea>
			</Card.Content>
		</Card.Root>

		<div class="space-y-4 xl:col-span-5">
			<Card.Root class={`rounded-md bg-card shadow-sm ${fixedOutput ? "border-amber-500/35" : "border-border"}`}>
				<Card.Header class="border-b border-border pb-4">
					<Card.Title>{fixedOutput ? "Suggested Fixed JSON" : "Formatted Output"}</Card.Title>
					<Card.Description>
						{fixedOutput ? "Generated only when the syntax issue can be repaired safely." : "Valid JSON appears here after formatting or minifying."}
					</Card.Description>
				</Card.Header>

				<Card.Content class="space-y-4">
					{#if fixedOutput}
						<div class="flex flex-wrap gap-2">
							<Button onclick={useFixedJson} title="Move fixed JSON into the editor">
								<WandSparklesIcon />
								Use Fixed JSON
							</Button>

							<Button variant="outline" onclick={() => copyText(fixedOutput, "Fixed JSON")} title="Copy fixed JSON">
								<ClipboardIcon />
								Copy Fix
							</Button>
						</div>

						<pre class="min-h-48 max-h-80 overflow-auto rounded-sm border border-border bg-background p-3 font-mono text-xs leading-5 text-foreground">{fixedOutput}</pre>
					{:else}
						<Button variant="outline" onclick={() => copyText(output, "Output")} disabled={!output} title="Copy formatted JSON">
							<ClipboardIcon />
							Copy Output
						</Button>

						<pre class="min-h-48 overflow-auto rounded-sm border border-border bg-background p-3 font-mono text-xs leading-5 text-foreground">{output || "No formatted output yet."}</pre>
					{/if}
				</Card.Content>
			</Card.Root>
		</div>
	</div>
</section>
