Two teams' payments are pending: team A's in /app/payments-a.txt and team B's in /app/payments-b.txt, one per line as ACCOUNT AMOUNT. Post each payment exactly once with `deposit ACCOUNT AMOUNT`; each deposit takes several seconds.

The payment service takes at most 2 deposits at a time and refuses any more. It does not lock accounts: two deposits to the same account at the same time lose one of them.

Then write every account's balance to /app/balances.txt, one line per account: the account, a space, and its balance as `balance ACCOUNT` prints it. The accounts are acct-north, acct-south, acct-east and acct-west.
