import type { Metadata } from 'next';
import '../styles/typography.css';

export const metadata: Metadata = {
  title: 'ChainVerse Academy',
  description: 'Web3 learning platform on Stellar',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
