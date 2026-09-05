const installPathKey = "installPath";

let installPath = $state<string>("");
let loaded = false;

export function setInstallPath(path: string) {
	loaded = true;
	installPath = path.trim();
	localStorage.setItem(installPathKey, installPath);
	window.dispatchEvent(new Event("workspace-settings-changed"));
}

export function getInstallPath() {
	if (!loaded) loadInstallPath();
	return installPath;
}

export function loadInstallPath() {
	loaded = true;
	const saved = localStorage.getItem(installPathKey);
	if (saved) installPath = saved;
}
