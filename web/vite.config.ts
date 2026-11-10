import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		// One page that finds its own way from the address: crucible-server
		// serves `build/` and answers anything else with index.html.
		sveltekit({ adapter: adapter({ fallback: 'index.html' }) })
	],
	// `npm run dev` talks to a crucible-server running beside it.
	server: { proxy: { '/api': 'http://127.0.0.1:8000' } }
});
