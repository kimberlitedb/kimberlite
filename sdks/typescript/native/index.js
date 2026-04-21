// Detects platform + arch at runtime and loads the matching prebuilt N-API
// addon from `./kimberlite-node.<triple>.node`. The published package bundles
// all supported triples so no compile step is required on the user's machine.

'use strict';

const { platform, arch } = process;

function triple() {
  switch (platform) {
    case 'darwin':
      if (arch === 'arm64') return 'darwin-arm64';
      if (arch === 'x64') return 'darwin-x64';
      break;
    case 'linux':
      // TODO: detect musl vs gnu via process.report.getReport(). For now we
      // assume gnu; musl builds ship under a separate triple that CI will
      // copy into this same folder.
      if (arch === 'arm64') return 'linux-arm64-gnu';
      if (arch === 'x64') return 'linux-x64-gnu';
      break;
    case 'win32':
      if (arch === 'x64') return 'win32-x64-msvc';
      break;
  }
  throw new Error(
    `Unsupported platform/arch for @kimberlitedb/client: ${platform}/${arch}. ` +
      `Supported: darwin-arm64, darwin-x64, linux-x64-gnu, linux-arm64-gnu, win32-x64-msvc.`,
  );
}

const t = triple();
const addonPath = `./kimberlite-node.${t}.node`;

let nativeBinding;
try {
  nativeBinding = require(addonPath);
} catch (e) {
  throw new Error(
    `Failed to load @kimberlitedb/client native addon at ${addonPath} (${t}): ${e.message}.\n` +
      `Re-run \`npm install\`, or rebuild locally with \`npm run build:native\` from the SDK source tree.`,
  );
}

module.exports = nativeBinding;
