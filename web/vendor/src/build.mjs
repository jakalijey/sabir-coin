// Rebuilds ../wallet-selector.js and ../wallet-selector.css.
// npm i --save-exact @near-wallet-selector/core@10.1.4 @near-wallet-selector/modal-ui@10.1.4 @near-wallet-selector/my-near-wallet@10.1.4 @near-wallet-selector/meteor-wallet@10.1.4 @near-wallet-selector/here-wallet@10.1.4 esbuild@0.25.9 buffer@6.0.3 process@0.11.10 util@0.12.5
// node build.mjs
import { build } from 'esbuild';
import { copyFileSync } from 'node:fs';

await build({
  entryPoints: ['entry.js'],
  bundle: true,
  format: 'esm',
  platform: 'browser',
  target: 'es2020',
  minify: true,
  legalComments: 'external',
  inject: ['shim.js'],
  define: { global: 'globalThis', 'process.env.NODE_ENV': '"production"' },
  alias: { crypto: './crypto-stub.js', util: 'util' },
  outfile: '../wallet-selector.js',
  logLevel: 'error',
});
copyFileSync('node_modules/@near-wallet-selector/modal-ui/styles.css', '../wallet-selector.css');
