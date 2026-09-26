export interface DependencyResource {
	name: string;
	path: string;
	origin: "data" | "artifact";
	dependencies: string[];
	provides: string[];
	groups: string[];
	dynamic: boolean;
}

export interface DependencyInventory {
	resources: DependencyResource[];
	resourcesIncomplete: boolean;
	configsIncomplete: boolean;
	startup: { command: "start" | "ensure" | "stop" | "restart"; target: string }[];
	warnings: { code: string; detail: string }[];
}

export interface DependencyRequest {
	artifactPath: string;
	txDataPath: string;
	profile: string;
}

export type DependencyResolution = {
	name: string;
	kind: "resolved" | "ambiguous" | "missing" | "unresolved" | "constraint";
	targets: string[];
};

export interface ResourceGraph {
	inventory: DependencyInventory;
	nodes: Map<string, DependencyResource>;
	declarations: Map<string, DependencyResolution[]>;
	dependencies: Map<string, Set<string>>;
	dependents: Map<string, Set<string>>;
	possibleDependencies: Map<string, Set<string>>;
	possibleDependents: Map<string, Set<string>>;
	cycles: string[][];
	truncated: boolean;
}

const nameKey = (name: string) => name.replace(/[A-Z]/g, (value) => value.toLowerCase());

export function resourcePathKey(path: string): string {
	const normalized = path.replace(/\\/g, "/").replace(/^\/\/\?\/UNC\//i, "//").replace(/^\/\/\?\//, "").replace(/\/+$/, "");
	return /^(?:[a-z]:|\/\/)/i.test(normalized) ? nameKey(normalized) : normalized;
}

export function buildResourceGraph(inventory: DependencyInventory): ResourceGraph {
	const nodes = new Map(inventory.resources.map((resource) => [resourcePathKey(resource.path), resource]));
	const ids = [...nodes.keys()].sort();
	const empty = () => new Map(ids.map((id) => [id, new Set<string>()]));
	const graph: ResourceGraph = {
		inventory, nodes, declarations: new Map(), dependencies: empty(), dependents: empty(),
		possibleDependencies: empty(), possibleDependents: empty(), cycles: [], truncated: false,
	};
	const implementations = new Map<string, Set<string>>();
	for (const [id, resource] of nodes) {
		for (const name of [resource.name, ...resource.provides]) {
			if (name.startsWith("/")) continue;
			const key = nameKey(name);
			if (!implementations.has(key)) implementations.set(key, new Set());
			implementations.get(key)!.add(id);
		}
	}
	// Ambiguous providers are possible impact only, never a definitive startup edge.
	let edgeBudget = 100_000;
	for (const id of ids) {
		const entries: DependencyResolution[] = [];
		for (const name of new Set(nodes.get(id)!.dependencies)) {
			if (name.startsWith("/")) { entries.push({ name, kind: "constraint", targets: [] }); continue; }
			const candidates = implementations.get(nameKey(name));
			const count = candidates?.size ?? 0;
			const targets: string[] = [];
			if (candidates) {
				for (const target of candidates) {
					if (edgeBudget-- <= 0) { graph.truncated = true; break; }
					targets.push(target);
					graph.possibleDependencies.get(id)!.add(target);
					graph.possibleDependents.get(target)!.add(id);
					if (count === 1) {
						graph.dependencies.get(id)!.add(target);
						graph.dependents.get(target)!.add(id);
					}
				}
			}
			entries.push({ name, targets, kind: count === 0 ? (inventory.resourcesIncomplete ? "unresolved" : "missing")
				: count > 1 ? "ambiguous" : targets.length ? "resolved" : "unresolved" });
		}
		graph.declarations.set(id, entries);
	}
	graph.cycles = findCycles(graph.dependencies, graph.dependents);
	return graph;
}

function reachable(edges: Map<string, Set<string>>, roots: Iterable<string>): Set<string> {
	const initial = new Set(roots);
	const visited = new Set(initial);
	const pending = [...initial];
	for (let i = 0; i < pending.length; i++) {
		for (const target of edges.get(pending[i]) ?? []) {
			if (!visited.has(target)) { visited.add(target); pending.push(target); }
		}
	}
	for (const root of initial) visited.delete(root);
	return visited;
}

// Iterative Kosaraju avoids exhausting the JS stack on long resource chains.
function findCycles(edges: Map<string, Set<string>>, reverse: Map<string, Set<string>>): string[][] {
	const visited = new Set<string>();
	const finished: string[] = [];
	for (const root of edges.keys()) {
		if (visited.has(root)) continue;
		visited.add(root);
		const stack = [{ id: root, children: edges.get(root)!.values() }];
		while (stack.length) {
			const frame = stack[stack.length - 1];
			const next = frame.children.next();
			if (next.done) { finished.push(frame.id); stack.pop(); }
			else if (!visited.has(next.value)) {
				visited.add(next.value);
				stack.push({ id: next.value, children: edges.get(next.value)!.values() });
			}
		}
	}
	visited.clear();
	const cycles: string[][] = [];
	for (const root of finished.reverse()) {
		if (visited.has(root)) continue;
		const component: string[] = [];
		const pending = [root];
		visited.add(root);
		while (pending.length) {
			const id = pending.pop()!;
			component.push(id);
			for (const target of reverse.get(id) ?? []) {
				if (!visited.has(target)) { visited.add(target); pending.push(target); }
			}
		}
		if (component.length > 1 || edges.get(root)?.has(root)) cycles.push(component.sort());
	}
	return cycles;
}

export function dependencyImpact(graph: ResourceGraph, path: string) {
	const id = resourcePathKey(path);
	const direct = new Set(graph.dependents.get(id) ?? []);
	direct.delete(id);
	const all = reachable(graph.dependents, [id]);
	const possible = reachable(graph.possibleDependents, [id]);
	const dependencies = reachable(graph.dependencies, [id]);
	return {
		id, resource: graph.nodes.get(id), direct: [...direct].sort(),
		transitive: [...all].filter((target) => !direct.has(target)).sort(),
		possible: [...possible].filter((target) => !all.has(target)).sort(),
		dependencies: [...dependencies].sort(),
		indirectDependencies: [...dependencies].filter((target) => !graph.dependencies.get(id)?.has(target)).sort(),
	};
}

export function dependencyStartupOrder(graph: ResourceGraph, scope?: Iterable<string>) {
	const included = new Set(scope ?? graph.nodes.keys());
	for (const id of reachable(graph.dependencies, included)) included.add(id);
	const blocked = new Set<string>();
	for (const id of included) {
		if (graph.nodes.get(id)?.dynamic || graph.declarations.get(id)?.some((entry) => ["missing", "unresolved", "ambiguous"].includes(entry.kind))) blocked.add(id);
	}
	for (const cycle of graph.cycles) for (const id of cycle) if (included.has(id)) blocked.add(id);
	for (const id of reachable(graph.dependents, blocked)) if (included.has(id)) blocked.add(id);
	const remaining = new Map<string, number>();
	for (const id of [...included].sort()) if (!blocked.has(id)) remaining.set(id, graph.dependencies.get(id)?.size ?? 0);
	const order = [...remaining].filter(([, count]) => count === 0).map(([id]) => id);
	for (let i = 0; i < order.length; i++) {
		for (const dependent of graph.dependents.get(order[i]) ?? []) {
			if (!remaining.has(dependent)) continue;
			const count = remaining.get(dependent)! - 1;
			remaining.set(dependent, count);
			if (count === 0) order.push(dependent);
		}
	}
	return { order, blocked: [...blocked].sort() };
}

export function dependencyRequestKey(request: DependencyRequest): string {
	return JSON.stringify([resourcePathKey(request.artifactPath), resourcePathKey(request.txDataPath), request.profile]);
}

export const DEPENDENCY_PAGE_SIZE = 50;

export function dependencyPage<T>(items: readonly T[], requestedPage: number) {
	const pages = Math.max(1, Math.ceil(items.length / DEPENDENCY_PAGE_SIZE));
	const page = Math.max(0, Math.min(pages - 1, Math.trunc(requestedPage) || 0));
	const start = page * DEPENDENCY_PAGE_SIZE;
	return { page, start, items: items.slice(start, start + DEPENDENCY_PAGE_SIZE) };
}

export function dependencyNames(graph: ResourceGraph, ids: readonly string[]): string {
	const shown = ids.slice(0, 8).map((id) => graph.nodes.get(id)?.name ?? id).join(", ");
	return ids.length > 8 ? `${shown} (8 of ${ids.length} shown)` : shown;
}
