import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { Table } from '@companyos/design-system';

describe('Table virtualisation', () => {
  beforeEach(() => {
    class RO {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal('ResizeObserver', RO);

    Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
      configurable: true,
      get() {
        return 320;
      },
    });
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
      configurable: true,
      get() {
        return 800;
      },
    });
    Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
      configurable: true,
      get() {
        return 320;
      },
    });
    Object.defineProperty(HTMLElement.prototype, 'offsetWidth', {
      configurable: true,
      get() {
        return 800;
      },
    });
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      width: 800,
      height: 320,
      top: 0,
      left: 0,
      bottom: 320,
      right: 800,
      x: 0,
      y: 0,
      toJSON() {
        return {};
      },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('mounts virtualizer for 250 rows and reports aria-rowcount', () => {
    const rows = Array.from({ length: 250 }, (_, i) => ({
      id: String(i),
      name: `Row ${i}`,
    }));

    const { container } = render(
      <Table
        maxHeight={320}
        estimateRowHeight={40}
        getRowKey={(r) => r.id}
        columns={[{ key: 'name', header: 'Name', cell: (r) => r.name }]}
        rows={rows}
      />,
    );

    const scroller = container.querySelector('[data-virtualized="true"]');
    expect(scroller).toBeTruthy();

    const counted = container.querySelector('[aria-rowcount="250"]');
    expect(counted).toBeTruthy();

    // Virtualised path should not mount all 250 body rows in the DOM.
    const bodyRows = container.querySelectorAll('tbody tr');
    expect(bodyRows.length).toBeLessThan(250);
  });
});
