import Container from "./chart-container.svelte";
import Tooltip from "./chart-tooltip.svelte";

export type ChartConfig = Record<
	string,
	{
		label?: string;
		color?: string;
	}
>;

export {
	Container,
	Tooltip,
	Container as ChartContainer,
	Tooltip as ChartTooltip,
};
