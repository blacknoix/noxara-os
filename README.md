Noxara OS
AI-native Business Operating System

Noxara OS is a unified operating system for running a company across sales, finance, people, operations, workflows, analytics, and AI on one shared platform instead of stitching together disconnected SaaS tools. It is designed around four core foundations: one governed data model, one permission system, one workflow engine, and one AI copilot that acts inside the same security and policy boundary as human users.

Vision
Most companies do not struggle because they lack software; they struggle because their work is fragmented across too many tools, too many logins, too many duplicated records, and too many brittle integrations. Noxara OS exists to replace that sprawl with a single multi-tenant, event-driven platform where core business functions share the same system of record, authorization model, workflow runtime, and AI surface.

Product thesis
Noxara OS is not another ERP assembled from loosely connected modules.[file:32] The product thesis is that a modern business platform should be built as an AI-native operating system where every state change is auditable, every workflow is durable, every permission decision is centralized, and every AI action uses the same APIs and authority model as the end user.

What Noxara OS includes
Core domains
Workspace: organizations, memberships, users, teams, departments, roles, permissions, settings.

Sales: leads, customers, contacts, pipelines, deals, activities, quotes, and later orders and contracts.

Finance: invoices, payments, allocations, expenses, journals, tax, FX, budgets, and accounting foundations.

Operations: projects, tasks, approvals, assets, inventory, procurement, and maintenance.

People: employees, attendance, leave, payroll, performance, and recruitment.

AI: copilot orchestration, retrieval, suggestions, document extraction, forecasting, and cost controls.

Admin and audit: audit logs, API keys, integrations, retention, export, and governance controls.

Shared platform capabilities
Multi-tenant isolation enforced in PostgreSQL with Row-Level Security, not only in application code.

One authorization implementation through a central authz layer used by gateway, services, workflows, and AI tools.
Event-driven integration through a transactional outbox and NATS JetStream.

Durable workflows for approvals, onboarding, dunning, imports, and long-running processes via Temporal.

Search, analytics, files, and notifications as first-class platform services.

Architecture
Noxara OS is a multi-tenant, event-driven, service-oriented platform with TypeScript/React clients and a Rust backend stack.[file:33] The primary shape is Next.js on web, Flutter on mobile, Tauri on desktop, Rust with Axum and SQLx for services, PostgreSQL as the system of record, Redis for ephemeral coordination, ClickHouse for analytics, OpenSearch for search, S3-compatible object storage, NATS JetStream for events, and Temporal for workflow orchestration.

Principles
Bounded contexts own their data; no service reads another service's tables directly.

Tenancy is enforced in the database and propagated through every layer, including cache keys, event subjects, search filters, and analytics predicates.

Every meaningful state change becomes an event.

Long-running business processes are workflows, not ad hoc cron jobs or status columns.

AI has no privileged bypass path.

Auditability, reversibility, and tenant isolation are non-negotiable from day one.

Phase roadmap
Phase	Scope	Target outcome
Phase 0	Foundations, repo, toolchain, local dev, CI/CD, infra baseline, shared crates, contract chain	A new engineer can clone the repo, boot the stack, ship a trivial PR, and deploy to staging.
Phase 1	Core platform and deal-to-cash	A design-partner org can sign up, manage workspace, run CRM, billing, projects, approvals, notifications, and an AI copilot in production.
Phase 2	People, money depth, and supply	HR, attendance, leave, payroll basics, accounting skeleton, inventory, procurement, and stronger governance posture.
Phase 3	Automation, analytics, API, marketplace	Customers can configure workflows, build reports, and integrate through a public API and SDKs.
Phase 4	Enterprise scale and autonomy	Multi-region, enterprise isolation, AI agents, low-code customization, industry modules, and client parity.
Phase 0 definition
Phase 0 is short but mandatory.It establishes the monorepo layout, shared crates, synthetic-data local environment, staging deployment path, observability baseline, infrastructure scaffolding, and the Rust-to-OpenAPI-to-TypeScript contract chain that the rest of the product depends on.

Definition of done
Nothing in Noxara OS is considered done unless it clears functionality, API contract, tenancy, authorization, audit events, tests, UI slice quality, observability, and documentation gates. This means every shipped capability must be tenant-safe, permission-tested, auditable, monitored, documented, and usable in real screens with loading, empty, error, stale, and permission-denied states.

Who it is for
Noxara OS starts with small and mid-market businesses that are outgrowing tool sprawl and operational fragmentation. The initial wedge is an integrated deal-to-cash and operating workflow for design partners, with later expansion into back-office depth, extensibility, and enterprise features.
Repository direction
This repository is the system source for:

Product docs and ADRs.

Monorepo applications and services.

Shared design system and SDK packages.

Infrastructure, deployment, and monitoring definitions.

API contracts, event schemas, and operational runbooks.

Initial repository layout
text
noxara-os/
├── apps/
│   ├── web/
│   ├── mobile/
│   ├── desktop/
│   └── admin/
├── services/
│   ├── auth-service/
│   ├── organization-service/
│   ├── user-service/
│   ├── crm-service/
│   ├── finance-service/
│   ├── project-service/
│   ├── workflow-service/
│   ├── notification-service/
│   ├── ai-service/
│   └── audit-service/
├── packages/
│   ├── design-system/
│   ├── ui/
│   ├── sdk/
│   ├── shared-types/
│   └── utils/
├── crates/
│   ├── authz/
│   ├── tenancy/
│   ├── events/
│   ├── outbox/
│   ├── telemetry/
│   ├── money/
│   ├── ids/
│   ├── errors/
│   └── testkit/
├── infrastructure/
├── docs/
└── scripts/
The repository shape follows the implementation plan and technical architecture so that service boundaries, shared contracts, and platform capabilities remain explicit from the first commit.

Early success criteria
The first major success state is not “features built”; it is ten design-partner organizations using Noxara OS as their primary system for the deal-to-cash loop for four consecutive weeks, with activation, reliability, security, operations, and documentation targets met.

Status
Current state: repo initialization and Phase 0 setup.

License
Proprietary until a formal license is published.

