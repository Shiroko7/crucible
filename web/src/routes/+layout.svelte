<script lang="ts">
	import { page } from '$app/state';
	import '../app.css';

	let { children } = $props();

	// The two shelves of the library, and the screens that belong to each.
	const shelves = [
		{ href: '/', label: 'Scenarios', under: ['/', '/scenario', '/build'] },
		{ href: '/creatures', label: 'Creatures', under: ['/creatures', '/creature'] }
	];
	const here = $derived(page.url.pathname.replace(/\/$/, '') || '/');
</script>

<header class="bar">
	<a class="brand" href="/">
		<svg viewBox="0 0 32 32" width="26" height="26" aria-hidden="true">
			<path class="flame" d="M16 3c2 4 6 6 6 11a6 6 0 0 1-12 0c0-2 1-3 2-4 0 2 1 3 2 3 0-4 0-6 2-10z" />
			<path class="bowl" d="M5 18h22c0 6-4 11-11 11S5 24 5 18z" />
		</svg>
		<span>Crucible</span>
	</a>
	<nav>
		{#each shelves as shelf (shelf.href)}
			<a href={shelf.href} class:here={shelf.under.includes(here)} data-test="shelf">
				{shelf.label}
			</a>
		{/each}
	</nav>
	<span class="tagline">Is this fight actually dangerous?</span>
</header>

{@render children()}

<style>
	.bar {
		display: flex;
		align-items: center;
		gap: 16px;
		padding: 9px 28px;
		border-bottom: 1px solid var(--border-strong);
		background: linear-gradient(180deg, #1c1814, #100e0c);
		box-shadow: 0 1px 0 #000;
		position: sticky;
		top: 0;
		z-index: 5;
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 10px;
		font: 600 18px var(--display);
		letter-spacing: 0.22em;
		text-transform: uppercase;
	}
	.flame {
		fill: var(--accent);
	}
	.bowl {
		fill: var(--surface-3);
		stroke: var(--muted);
		stroke-width: 1.5;
	}
	nav {
		display: flex;
		gap: 4px;
		margin-left: 12px;
	}
	nav a {
		padding: 5px 12px;
		font-size: 12px;
		letter-spacing: 0.14em;
		text-transform: uppercase;
		color: var(--muted);
		border-bottom: 2px solid transparent;
	}
	nav a:hover {
		color: var(--text);
	}
	nav a.here {
		color: var(--accent);
		border-bottom-color: var(--accent);
	}
	.tagline {
		margin-left: auto;
		color: var(--faint);
		font: italic 14px var(--display);
	}
	@media (max-width: 600px) {
		.tagline {
			display: none;
		}
	}
</style>
