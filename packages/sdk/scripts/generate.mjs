#!/usr/bin/env node
/**
 * Generate TypeScript types from openapi.json.
 * Full openapi-typescript can replace this later; CI drift check uses the committed JSON.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const openapi = JSON.parse(readFileSync(join(root, 'openapi.json'), 'utf8'));

function tsType(v) {
  if (!v) return 'unknown';
  if (v.$ref) {
    return v.$ref.split('/').pop();
  }
  if (v.type === 'array') {
    return `${tsType(v.items)}[]`;
  }
  if (v.type === 'integer' || v.type === 'number') return 'number';
  if (v.type === 'boolean') return 'boolean';
  if (v.type === 'object') {
    if (v.additionalProperties) return 'Record<string, unknown>';
    return 'Record<string, unknown>';
  }
  // utoipa emits bare `{}` for some serde_json::Value fields
  if (typeof v === 'object' && !v.type && !v.$ref && !v.anyOf && !v.oneOf) {
    return 'Record<string, unknown>';
  }
  return 'string';
}

function propsToTs(schema) {
  if (!schema?.properties) return '';
  const req = new Set(schema.required ?? []);
  return Object.entries(schema.properties)
    .map(([k, v]) => {
      const opt = req.has(k) ? '' : '?';
      const desc = v.description ? `  /** ${v.description} */\n` : '';
      return `${desc}  ${k}${opt}: ${tsType(v)};`;
    })
    .join('\n');
}

const schemas = openapi.components.schemas;
const emit = [
  'Hello',
  'CreateHelloRequest',
  'HelloListResponse',
  'RegisterRequest',
  'RegisterResponse',
  'LoginRequest',
  'TokenResponse',
  'MfaChallengeResponse',
  'SwitchOrgRequest',
  'MeResponse',
  'MembershipView',
  'MembershipListResponse',
  'SessionView',
  'SessionListResponse',
  'MessageResponse',
  'CreateOrgRequest',
  'OrgResponse',
  'UpdateOrgSettingsRequest',
  'MemberView',
  'MemberListResponse',
  'InviteMemberRequest',
  'InviteResponse',
  'RoleView',
  'RolePermissionView',
  'RoleListResponse',
  'UpsertRoleRequest',
  'RolePermissionInput',
  'CapabilityPreviewResponse',
  'PermissionCatalogueItem',
  'PermissionCatalogueResponse',
  'TeamView',
  'TeamListResponse',
  'DepartmentView',
  'DepartmentListResponse',
  'MyCapabilitiesResponse',
  'DashboardResponse',
  'DashboardWidget',
  'CustomerDto',
  'DealDto',
  'QuoteDto',
  'QuoteLineDto',
  'LeadDto',
  'BoardResponse',
  'BoardStage',
  'StageDto',
  'ReportSummaryResponse',
  'PipelineDto',
  'CreateDealRequest',
  'CreateCustomerRequest',
  'CreateQuoteRequest',
  'CreateQuoteLineRequest',
  'InvoiceActionResponse',
  'StageSummary',
  'WinRateSummary',
  'WeightedForecast',
  'ActivityVolumeItem',
  'InvoiceDto',
  'InvoiceLineDto',
  'InvoiceLineInput',
  'InvoiceListResponse',
  'CreateInvoiceRequest',
  'CreateInvoiceFromQuoteRequest',
  'QuoteLineSnapshot',
  'IssueInvoiceRequest',
  'PaymentDto',
  'RecordPaymentRequest',
  'CreditNoteDto',
  'CreateCreditNoteRequest',
  'ExpenseDto',
  'SubmitExpenseRequest',
  'ReportSummaryDto',
  'AgeingBucket',
  'CategoryAmount',
  'CashFlowPoint',
  'FinanceCustomerDto',
  'WebhookAck',
  'ProjectDto',
  'TaskDto',
  'TaskBoardResponse',
  'BoardColumnDto',
  'ChecklistItemDto',
  'TaskAttachmentDto',
  'TaskCommentDto',
  'MyWorkResponse',
  'SummaryResponse',
  'NotificationItemDto',
  'FeedResponse',
  'SearchHit',
  'PresignUploadRequest',
  'PresignUploadResponse',
  'FileMetaResponse',
];

const banner = `/** AUTO-GENERATED from openapi.json — do not edit by hand. Run pnpm generate:sdk */\n`;
const types = `${banner}
${emit
  .filter((name) => schemas[name])
  .map((name) => `export type ${name} = {\n${propsToTs(schemas[name])}\n};`)
  .join('\n\n')}
`;

writeFileSync(join(root, 'src/generated.ts'), types);
console.log(`wrote src/generated.ts (${emit.length} types)`);
