export const metadata = {
  title: 'RATA — Every message. Every language. One inbox.',
  description: 'Texts, email, Slack, and Discord — translated automatically and unified into one inbox.',
};

export default function RootLayout({ children }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
