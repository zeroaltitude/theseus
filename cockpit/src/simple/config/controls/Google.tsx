// Google, the control that is not here: Theseus's four Google methods (sign in, finish, status, take the access back) are
// answered by the daemon only to the command line on the machine it runs on, which refuses every other surface in these
// words: "the operator's, from the CLI on this machine". A page cannot do them, so this says where each is done.
import { KeyRound } from 'lucide-react'
import { Panel } from '@/components/ui'

export function Google() {
  return (
    <Panel help="simple.config.google" title="Google" icon={<KeyRound size={13} />} bodyClassName="flex flex-col gap-2 p-4 text-[13px] leading-snug text-ink-dim">
      <p>Signing in to Google, seeing what it was allowed to do, and taking its access back are done at the command line on the computer Theseus runs on, not on a page. Theseus itself refuses them from anywhere else, so that a page open in a browser can never grant or take away access to your Google account.</p>
      <ul className="list-disc pl-5">
        <li>Sign in: <code className="num text-ink">theseus google auth</code>. It prints Google’s sign-in address; open it on the computer Theseus runs on. If nothing arrives within 90 seconds, paste the address of the page that failed to load. <code className="num text-ink">--manual</code> goes straight to the paste.</li>
        <li>See what it may do: <code className="num text-ink">theseus google status</code>.</li>
        <li>Take the access back: <code className="num text-ink">theseus google revoke</code>.</li>
      </ul>
    </Panel>
  )
}
