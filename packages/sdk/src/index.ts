/** TypeScript SDK stub for CompanyOS Phase 0. */

export type { Hello, CreateHelloRequest, HelloListResponse } from './generated';
import type { Hello, CreateHelloRequest, HelloListResponse } from './generated';

export type CompanyOsClientOptions = {
  baseUrl: string;
  /** LOCAL-ONLY: org public id for X-CompanyOS-Dev-Org-Id */
  orgId?: string;
  /** LOCAL-ONLY: user public id or uuid for X-CompanyOS-Dev-User-Id */
  userId?: string;
  getHeaders?: () => Record<string, string>;
};

/**
 * Thin TypeScript SDK stub for CompanyOS Phase 0.
 * Auth headers are LOCAL-ONLY — never ship these to production clients as the sole auth.
 */
export class CompanyOsClient {
  constructor(private readonly opts: CompanyOsClientOptions) {}

  private headers(): Record<string, string> {
    const h: Record<string, string> = {
      Accept: 'application/json',
      'Content-Type': 'application/json',
      ...(this.opts.getHeaders?.() ?? {}),
    };
    if (this.opts.orgId) h['X-CompanyOS-Dev-Org-Id'] = this.opts.orgId;
    if (this.opts.userId) h['X-CompanyOS-Dev-User-Id'] = this.opts.userId;
    return h;
  }

  async listHello(): Promise<HelloListResponse> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/hello`, {
      method: 'GET',
      headers: this.headers(),
    });
    if (!res.ok) {
      throw new Error(`listHello failed: ${res.status}`);
    }
    return (await res.json()) as HelloListResponse;
  }

  async createHello(body: CreateHelloRequest): Promise<Hello> {
    const res = await fetch(`${this.opts.baseUrl}/api/v1/hello`, {
      method: 'POST',
      headers: this.headers(),
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      throw new Error(`createHello failed: ${res.status}`);
    }
    return (await res.json()) as Hello;
  }
}
