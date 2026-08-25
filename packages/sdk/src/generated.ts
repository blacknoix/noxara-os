/** AUTO-GENERATED from openapi.json — do not edit by hand. Run pnpm generate:sdk */

export type Hello = {
  /** Prefixed public id (`hel_…`). */
  id: string;
  /** Prefixed org id (`org_…`). */
  org_id: string;
  message: string;
  created_by: string;
};

export type CreateHelloRequest = {
  message: string;
};

export type HelloListResponse = {
  items: Hello[];
};
