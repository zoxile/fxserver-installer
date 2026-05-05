export let logs = $state<string[]>([]);

export function log(message: string) {
	logs.push(`fxserver-installer [${new Date().toLocaleTimeString()}] ${message}`);
}
