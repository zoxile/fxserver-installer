let installPath = $state<string>("");

export function setInstallPath(path: string) {
	installPath = path;
	localStorage.setItem("installPath", path);
}

export function getInstallPath() {
	return installPath;
}

export function loadInstallPath() {
	const saved = localStorage.getItem("installPath");
	if (saved) installPath = saved;
}
