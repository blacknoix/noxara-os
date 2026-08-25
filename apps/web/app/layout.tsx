import type { ReactNode } from 'react';
import { AppShell } from '@/components/AppShell';
import '@companyos/design-system/tokens.css';
import './globals.css';

export const metadata = {
  title: 'CompanyOS',
  description: 'AI-native Business Operating System',
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" data-theme="light">
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="" />
        <link
          href="https://fonts.googleapis.com/css2?family=Fraunces:opsz,wght@9..144,500;9..144,650&family=Sora:wght@400;500;600&display=swap"
          rel="stylesheet"
        />
      </head>
      <body>
        <AppShell>{children}</AppShell>
      </body>
    </html>
  );
}
