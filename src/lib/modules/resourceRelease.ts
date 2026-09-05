export interface ResourceRelease { title: string; tag: string; body: string; url: string; publishedAt: string | null }
const cache = new Map<string, { value: ResourceRelease | null; fetchedAt: number }>();

export function githubReleaseRepository(repository: string) {
	const value = repository.replace(/^git@github\.com:/i, "https://github.com/").replace(/\.git\/?$/, "");
	try {
		const url = new URL(value);
		if (url.protocol !== "https:" || url.hostname !== "github.com" || url.username || url.password) return null;
		const parts = url.pathname.split("/").filter(Boolean);
		if (parts.length < 2 || !parts.slice(0, 2).every((part) => /^[\w.-]+$/.test(part))) return null;
		return `${parts[0]}/${parts[1]}`;
	} catch { return null; }
}

export async function fetchResourceRelease(repository: string, refresh = false): Promise<ResourceRelease | null> {
	const slug = githubReleaseRepository(repository);
	if (!slug) throw new Error("Release notes require an official GitHub repository URL.");
	const cached = cache.get(slug.toLowerCase());
	if (!refresh && cached && Date.now() - cached.fetchedAt < 15 * 60 * 1000) return cached.value;
	const response = await fetch(`https://api.github.com/repos/${slug}/releases/latest`, { headers: { Accept: "application/vnd.github+json" }, cache: "no-store", signal: AbortSignal.timeout(20000) });
	if (response.status === 404) { cache.set(slug.toLowerCase(), { value: null, fetchedAt: Date.now() }); return null; }
	if (response.status === 403 || response.status === 429) throw new Error("GitHub rate-limited release notes. The repository release link is still available.");
	if (!response.ok) throw new Error(`GitHub returned ${response.status} while loading release notes.`);
	const data = await response.json();
	if (typeof data.tag_name !== "string" || typeof data.html_url !== "string") throw new Error("GitHub returned invalid release metadata.");
	const url = new URL(data.html_url);
	if (url.origin !== "https://github.com" || !url.pathname.startsWith(`/${slug}/releases/`)) throw new Error("GitHub returned an unexpected release link.");
	const value: ResourceRelease = { title: typeof data.name === "string" && data.name ? data.name : data.tag_name, tag: data.tag_name,
		body: typeof data.body === "string" ? data.body.slice(0, 100000) : "", url: url.href, publishedAt: typeof data.published_at === "string" ? data.published_at : null };
	cache.set(slug.toLowerCase(), { value, fetchedAt: Date.now() });
	return value;
}
