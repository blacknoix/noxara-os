Noxara OS
AI-native Business Operating System[file:32]

Noxara OS is a unified operating system for running a company across sales, finance, people, operations, workflows, analytics, and AI on one shared platform instead of stitching together disconnected SaaS tools.[file:32] It is designed around four core foundations: one governed data model, one permission system, one workflow engine, and one AI copilot that acts inside the same security and policy boundary as human users.[file:32][file:33]

Vision
Most companies do not struggle because they lack software; they struggle because their work is fragmented across too many tools, too many logins, too many duplicated records, and too many brittle integrations.[file:32] Noxara OS exists to replace that sprawl with a single multi-tenant, event-driven platform where core business functions share the same system of record, authorization model, workflow runtime, and AI surface.[file:32][file:33]

Product thesis
Noxara OS is not another ERP assembled from loosely connected modules.[file:32] The product thesis is that a modern business platform should be built as an AI-native operating system where every state change is auditable, every workflow is durable, every permission decision is centralized, and every AI action uses the same APIs and authority model as the end user.[file:33]

What Noxara OS includes
Core domains
Workspace: organizations, memberships, users, teams, departments, roles, permissions, settings.[file:33]

Sales: leads, customers, contacts, pipelines, deals, activities, quotes, and later orders and contracts.[file:33]

Finance: invoices, payments, allocations, expenses, journals, tax, FX, budgets, and accounting foundations.[file:33]

Operations: projects, tasks, approvals, assets, inventory, procurement, and maintenance.[file:33]

People: employees, attendance, leave, payroll, performance, and recruitment.[file:33]

AI: copilot orchestration, retrieval, suggestions, document extraction, forecasting, and cost controls.[file:33]

Admin and audit: audit logs, API keys, integrations, retention, export, and governance controls.[file:33]

Shared platform capabilities
Multi-tenant isolation enforced in PostgreSQL with Row-Level Security, not only in application code.[file:33]

One authorization implementation through a central authz layer used by gateway, services, workflows, and AI tools.[file:33]

Event-driven integration through a transactional outbox and NATS JetStream.[file:33]

Durable workflows for approvals, onboarding, dunning, imports, and long-running processes via Temporal.[file:33]

Search, analytics, files, and notifications as first-class platform services.[file:33]

Architecture
Noxara OS is a multi-tenant, event-driven, service-oriented platform with TypeScript/React clients and a Rust backend stack.[file:33] The primary shape is Next.js on web, Flutter on mobile, Tauri on desktop, Rust with Axum and SQLx for services, PostgreSQL as the system of record, Redis for ephemeral coordination, ClickHouse for analytics, OpenSearch for search, S3-compatible object storage, NATS JetStream for events, and Temporal for workflow orchestration.[file:33]

Principles
Bounded contexts own their data; no service reads another service's tables directly.[file:33]

Tenancy is enforced in the database and propagated through every layer, including cache keys, event subjects, search filters, and analytics predicates.[file:33]

Every meaningful state change becomes an event.[file:33]

Long-running business processes are workflows, not ad hoc cron jobs or status columns.[file:33]

AI has no privileged bypass path.[file:33]

Auditability, reversibility, and tenant isolation are non-negotiable from day one.[file:33][file:37]

Phase roadmap
Phase	Scope	Target outcome
Phase 0	Foundations, repo, toolchain, local dev, CI/CD, infra baseline, shared crates, contract chain	A new engineer can clone the repo, boot the stack, ship a trivial PR, and deploy to staging.[file:37]
Phase 1	Core platform and deal-to-cash	A design-partner org can sign up, manage workspace, run CRM, billing, projects, approvals, notifications, and an AI copilot in production.[file:37]
Phase 2	People, money depth, and supply	HR, attendance, leave, payroll basics, accounting skeleton, inventory, procurement, and stronger governance posture.[file:37]
Phase 3	Automation, analytics, API, marketplace	Customers can configure workflows, build reports, and integrate through a public API and SDKs.[file:37]
Phase 4	Enterprise scale and autonomy	Multi-region, enterprise isolation, AI agents, low-code customization, industry modules, and client parity.[file:37]
Phase 0 definition
Phase 0 is short but mandatory.[file:37] It establishes the monorepo layout, shared crates, synthetic-data local environment, staging deployment path, observability baseline, infrastructure scaffolding, and the Rust-to-OpenAPI-to-TypeScript contract chain that the rest of the product depends on.[file:37]

Definition of done
Nothing in Noxara OS is considered done unless it clears functionality, API contract, tenancy, authorization, audit events, tests, UI slice quality, observability, and documentation gates.[file:37] This means every shipped capability must be tenant-safe, permission-tested, auditable, monitored, documented, and usable in real screens with loading, empty, error, stale, and permission-denied states.[file:37]

Who it is for
Noxara OS starts with small and mid-market businesses that are outgrowing tool sprawl and operational fragmentation.[file:32] The initial wedge is an integrated deal-to-cash and operating workflow for design partners, with later expansion into back-office depth, extensibility, and enterprise features.[file:32][file:37]

Repository direction
This repository is the system source for:

Product docs and ADRs.

Monorepo applications and services.

Shared design system and SDK packages.

Infrastructure, deployment, and monitoring definitions.

API contracts, event schemas, and operational runbooks.[file:37][file:33]

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
The repository shape follows the implementation plan and technical architecture so that service boundaries, shared contracts, and platform capabilities remain explicit from the first commit.[file:33][file:37]

Early success criteria
The first major success state is not “features built”; it is ten design-partner organizations using Noxara OS as their primary system for the deal-to-cash loop for four consecutive weeks, with activation, reliability, security, operations, and documentation targets met.[file:37]

Status
Current state: repo initialization and Phase 0 setup.[file:37]

License
Proprietary until a formal license is published.

