export const metadata = {
  /* The same pitch as public/index.html's title and description. */
  title: 'RATA · Your mail, on your machine',
  description: 'RATA is a desktop mail client for Windows, macOS and Linux that connects to your mailbox directly. Your password and your mail stay on your computer.',
};

export default function RootLayout({ children }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
