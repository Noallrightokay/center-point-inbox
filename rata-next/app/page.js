import { redirect } from 'next/navigation';

// Fallback if middleware is bypassed: land on the RATA landing page.
export default function Home() {
  redirect('/index.html');
}
