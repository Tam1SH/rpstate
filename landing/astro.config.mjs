// @ts-check

import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'astro/config';

import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
  site: 'https://uniproc-dev.github.io',
  base: '/amethystate',

  vite: {
      plugins: [tailwindcss()],
	},

  integrations: [starlight({
      title: 'amethystate',
      customCss: ['./src/styles/starlight.css'],
  })],
});