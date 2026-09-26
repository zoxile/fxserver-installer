import { invoke } from "@tauri-apps/api/core";

interface BackupReservation { token: string; expiresAt: number }

export async function reserveMaintenanceBackups(workspaceId: string, scheduleIds: string[]) {
	const reservation = await invoke<BackupReservation>("reserve_maintenance_backups", { workspaceId, scheduleIds });
	let expiresAt = reservation.expiresAt;
	let closed = false;
	let failed = false;
	let timer: ReturnType<typeof setTimeout> | undefined;
	let pending: Promise<void> | undefined;
	const args = { workspaceId, token: reservation.token };

	function assertActive() {
		if (closed || failed || Date.now() >= expiresAt) {
			throw new Error("The maintenance backup reservation expired or could not be renewed. Review the server state before retrying.");
		}
	}

	function scheduleRenewal() {
		timer = setTimeout(() => {
			pending = invoke<BackupReservation>("renew_maintenance_backup_reservation", args)
				.then((receipt) => { expiresAt = receipt.expiresAt; })
				.catch(() => { failed = true; })
				.finally(() => {
					pending = undefined;
					if (!closed && !failed) scheduleRenewal();
				});
		}, 30_000);
	}
	scheduleRenewal();

	return {
		token: reservation.token,
		assertActive,
		async release() {
			closed = true;
			clearTimeout(timer);
			await pending;
			await invoke("release_maintenance_backup_reservation", args);
		},
	};
}
