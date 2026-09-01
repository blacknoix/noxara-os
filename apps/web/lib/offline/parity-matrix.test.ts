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
    expect(md).toContain('not-yet');
    expect(md).toContain('last-write-wins');
    expect(md).toContain('If-Match');
  });
});
