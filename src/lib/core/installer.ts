export type StepStatus = "pending" | "running" | "done" | "error";

export let steps = $state<{ name: string; status: StepStatus }[]>([]);

export function setSteps(names: string[]) {
	steps = names.map((n) => ({ name: n, status: "pending" }));
}

export function updateStep(i: number, status: StepStatus) {
	steps[i].status = status;
}
