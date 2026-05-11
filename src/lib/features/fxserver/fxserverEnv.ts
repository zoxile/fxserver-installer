export interface TxHostField {
	key: string;
	label: string;
	description: string;
	placeholder: string;
	group: "General" | "Networking" | "Provider" | "Defaults";
	type?: "text" | "number" | "password" | "url" | "select";
	options?: string[];
}

export const txHostFields: TxHostField[] = [
	{
		key: "TXHOST_DATA_PATH",
		label: "Data Path",
		description: "txData folder for logs, configs, recipes, and deployed servers.",
		placeholder: "C:\\FXServer\\txData",
		group: "General",
	},
	{
		key: "TXHOST_GAME_NAME",
		label: "Game",
		description: "Restrict setup to FiveM or RedM recipes.",
		placeholder: "fivem",
		group: "General",
		type: "select",
		options: ["", "fivem", "redm"],
	},
	{
		key: "TXHOST_MAX_SLOTS",
		label: "Max Slots",
		description: "Maximum player slots txAdmin will allow in server.cfg.",
		placeholder: "48",
		group: "General",
		type: "number",
	},
	{
		key: "TXHOST_QUIET_MODE",
		label: "Quiet Mode",
		description: "Stops FXServer stdout/stderr from being piped into txAdmin stdout.",
		placeholder: "false",
		group: "General",
		type: "select",
		options: ["", "false", "true"],
	},
	{
		key: "TXHOST_API_TOKEN",
		label: "API Token",
		description: "Enables /host/status using disabled or a 16-48 character token.",
		placeholder: "disabled",
		group: "General",
		type: "password",
	},
	{
		key: "TXHOST_TXA_URL",
		label: "txAdmin URL",
		description: "Public URL shown by txAdmin in the boot message.",
		placeholder: "https://panel.example.com",
		group: "Networking",
		type: "url",
	},
	{
		key: "TXHOST_TXA_PORT",
		label: "txAdmin Port",
		description: "TCP port txAdmin binds to. Cannot be 30120.",
		placeholder: "40120",
		group: "Networking",
		type: "number",
	},
	{
		key: "TXHOST_FXS_PORT",
		label: "FXServer Port",
		description: "Forces endpoint_add_* commands in server.cfg to this port. Cannot be 40120.",
		placeholder: "30120",
		group: "Networking",
		type: "number",
	},
	{
		key: "TXHOST_INTERFACE",
		label: "Interface",
		description: "Interface txAdmin binds to and enforces for FXServer.",
		placeholder: "0.0.0.0",
		group: "Networking",
	},
	{
		key: "TXHOST_PROVIDER_NAME",
		label: "Provider Name",
		description: "Short host label shown in txAdmin warnings.",
		placeholder: "Host Config",
		group: "Provider",
	},
	{
		key: "TXHOST_PROVIDER_LOGO",
		label: "Provider Logo",
		description: "Logo URL shown on the txAdmin login page.",
		placeholder: "https://example.com/logo_{theme}.png",
		group: "Provider",
		type: "url",
	},
	{
		key: "TXHOST_DEFAULT_CFXKEY",
		label: "Default CFX Key",
		description: "Autofills the Cfx.re key during txAdmin deployment.",
		placeholder: "cfxk_...",
		group: "Defaults",
		type: "password",
	},
	{
		key: "TXHOST_DEFAULT_DBHOST",
		label: "DB Host",
		description: "Autofills database host during deployer setup.",
		placeholder: "127.0.0.1",
		group: "Defaults",
	},
	{
		key: "TXHOST_DEFAULT_DBPORT",
		label: "DB Port",
		description: "Autofills database port during deployer setup.",
		placeholder: "3306",
		group: "Defaults",
		type: "number",
	},
	{
		key: "TXHOST_DEFAULT_DBUSER",
		label: "DB User",
		description: "Autofills database username during deployer setup.",
		placeholder: "root",
		group: "Defaults",
	},
	{
		key: "TXHOST_DEFAULT_DBPASS",
		label: "DB Password",
		description: "Autofills database password during deployer setup.",
		placeholder: "password",
		group: "Defaults",
		type: "password",
	},
	{
		key: "TXHOST_DEFAULT_DBNAME",
		label: "DB Name",
		description: "Autofills database name during deployer setup.",
		placeholder: "server_db",
		group: "Defaults",
	},
	{
		key: "TXHOST_DEFAULT_ACCOUNT",
		label: "Default Account",
		description: "Optional username:FiveM ID:bcrypt hash bootstrap account.",
		placeholder: "tabarra:271816",
		group: "Defaults",
		type: "password",
	},
];

export const txHostGroups = ["General", "Networking", "Provider", "Defaults"] as const;
