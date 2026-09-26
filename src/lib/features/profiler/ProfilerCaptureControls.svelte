<script lang="ts">
	import CircleIcon from "@lucide/svelte/icons/circle";
	import SquareIcon from "@lucide/svelte/icons/square";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Progress } from "$lib/components/ui/progress/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { CAPTURE_FRAME_OPTIONS, cancelProfilerCapture, profilerCaptureSession as session, startProfilerCapture } from "./profilerCapture.svelte";
	const id = $props.id();
	const frameOptions = CAPTURE_FRAME_OPTIONS.map((value) => ({ value: String(value), label: `${value} frames` }));
	let frames = $state(session.requestedFrames);
	const phase = $derived(({ checking: "Checking profiler", recording: "Recording", saving: "Saving JSON", cleaning: "Finishing export cleanup", analyzing: "Analyzing capture", completed: "Capture complete", cancelled: "Capture cancelled", failed: "Capture failed", idle: "Ready" } as Record<string, string>)[session.phase] ?? session.phase);
</script>

<section class="space-y-3 border-y border-border py-4" aria-label="Guided performance recording">
	<div class="flex flex-wrap items-end justify-between gap-4">
		<div class="min-w-0">
			<h2 class="text-base font-semibold">Guided Performance Recording</h2>
			<p class="mt-1 text-xs text-muted-foreground">Managed local FXServer / RCON / 16 MiB maximum export</p>
		</div>
		<div class="flex flex-wrap items-end gap-2">
			<div class="grid gap-1 text-xs text-muted-foreground">
				<label for={`${id}-frames`}>Frames</label>
				<Select.Root type="single" value={String(frames)} items={frameOptions} disabled={session.busy} onValueChange={(value) => { if (frameOptions.some((option) => option.value === value)) frames = Number(value); }}>
					<Select.Trigger id={`${id}-frames`} aria-label="Frames" class="w-32 text-foreground">{frames} frames</Select.Trigger>
					<Select.Content>
						{#each frameOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}
					</Select.Content>
				</Select.Root>
			</div>
			{#if session.busy}
				<Button variant="outline" disabled={session.cancelRequested} onclick={() => void cancelProfilerCapture()}><SquareIcon class="size-4" />{session.cancelRequested ? "Cancelling" : "Cancel capture"}</Button>
			{:else}
				<Button disabled={session.analyzing} onclick={() => void startProfilerCapture(frames)}><CircleIcon class="size-4 text-red-300" />Record capture</Button>
			{/if}
		</div>
	</div>
	{#if session.busy}
		<div class="flex items-center gap-3 text-xs" role="status">
			<LoaderCircleIcon class="size-4 shrink-0 animate-spin" />
			<span>{phase}{session.phase === "recording" ? `: ${session.capturedFrames} / ${session.requestedFrames} frames` : ""}</span>
		</div>
		<Progress value={session.requestedFrames ? session.capturedFrames / session.requestedFrames * 100 : 0} class="h-1.5" />
		{#if session.cancelRequested}<p class="text-xs text-muted-foreground">Cancellation does not stop the server profiler. An export already sent has a bounded completion wait; uncertain output stays protected and is reported for cleanup on a later capture request.</p>{/if}
	{/if}
	{#if session.error}<p role="alert" class="text-sm text-destructive wrap-anywhere">{session.error}</p>{/if}
	<p class="text-xs text-muted-foreground">Frames are not seconds. The recording wait has a 45-second deadline; cancellation and timeout never send a profiler stop command. A started recording ends as the server advances through its requested frames. Profiling adds overhead and replaces any unsaved, finished profiler buffer.</p>
</section>
