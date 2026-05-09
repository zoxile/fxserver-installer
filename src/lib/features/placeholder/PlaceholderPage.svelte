<script lang="ts">
	import ConstructionIcon from "@lucide/svelte/icons/construction";
	import * as Card from "$lib/components/ui/card/index.js";

	type Props = {
		title: string;
		description: string;
	};

	let { title, description }: Props = $props();
</script>

<section class="space-y-6">
	<div>
		<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Coming Up</p>
		<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">{title}</h1>
		<p class="mt-2 max-w-2xl text-sm text-muted-foreground">{description}</p>
	</div>

	<Card.Root class="rounded-md border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<div class="flex items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<ConstructionIcon class="size-4" />
				</div>
				<div>
					<Card.Title>Placeholder</Card.Title>
					<Card.Description>This page is wired into navigation and ready for its feature implementation.</Card.Description>
				</div>
			</div>
		</Card.Header>
		<Card.Content>
			<div class="rounded-sm border border-dashed border-border bg-background p-4 text-sm text-muted-foreground">
				{description}
			</div>
		</Card.Content>
	</Card.Root>
</section>
