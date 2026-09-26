<script lang="ts">
	import { untrack } from "svelte";
	import { profilerCaptureSession } from "./profilerCapture.svelte";
	let { onopen }: { onopen: () => void } = $props();
	let handled = $state(0);
	// Mount once in App, outside keyed pages. Navigation never owns the recording task.
	$effect(() => {
		const request = profilerCaptureSession.openRequest;
		if (request > handled) {
			handled = request;
			untrack(onopen);
		}
	});
</script>
