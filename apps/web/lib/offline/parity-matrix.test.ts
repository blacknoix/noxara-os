import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const MATRIX = resolve(__dirname, '../../../../docs/clients/parity-matrix.md');

describe('client parity matrix', () => {
  it('exists and documents web vs mobile vs desktop for the 1.11 surface', () => {
    const md = readFileSync(MATRIX, 'utf8');
    expect(md).toContain('# Client parity matrix');
    for (const feature of [
      'Auth',
      'Org switch',
      'Dashboard',
      'Approvals',
      'Tasks',
      'Deal quick-update',
      'Expense capture',
      'Copilot',
    ]) {
      expect(md, `missing feature row: ${feature}`).toContain(feature);
    }
    expect(md).toContain('implemented');
    expect(md).toContain('last-write-wins');
    expect(md).toContain('If-Match');
    // Phase 1.11 shipped Flutter + Tauri shells for the high-frequency set.
    expect(md).toContain('apps/mobile');
    expect(md).toContain('apps/desktop');
    expect(md).toContain('companyos://record/');
    expect(md).toContain('FakeBiometricService');
    expect(md).toContain('out-of-scope');
    expect(md).toContain('android-signed-release');
    expect(md).toContain('FakeCrashTransport');
    expect(md).toContain('store-release.md');
    // Auth row: web + mobile both implemented
    expect(md).toMatch(/\|\s*Auth[^\n]*\|\s*implemented\s*\|\s*implemented\s*\|/);
  });
});
