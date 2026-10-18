# Privacy

Updated: 2026-10-18

## What Theseus is

Theseus is personal, self-hosted software. One owner runs it on their own machine. There is no Theseus service, server
or company that receives anyone's data: the source is this repository, and nothing in it sends data to its authors.

## What Google data it reads, and why

Only what the owner grants it on Google's own consent screen (for example mail, calendar or files, when the owner turns
that integration on), and only to do what the owner has asked it to do: answer a question, run a task, or fill a
routine the owner set up.

## Where it stays

On the owner's machine, in Theseus's own store. It is never sold, never shared, never used for advertising, and never
used to train models.

## Who else sees it

The parts of it that a turn needs are sent to the model providers the owner has configured, for that turn only, under
those providers' own terms. Google Workspace content is never sent to models of China origin. That is the owner's stated
policy, and this page states it.

## Google API Services User Data Policy

Theseus's use of data received from Google APIs adheres to the
[Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including
the Limited Use requirements.

## How to revoke access

- At Google: open <https://myaccount.google.com/permissions> and remove Theseus. Access ends at once.
- On the machine: delete the stored token. Theseus keeps credentials in its owner's secret store, named by reference in
  its config, as it keeps every other credential; [setup.md](setup.md) says how that store is set up. Removing the entry
  there removes the token.

## Contact

Open an issue at <https://github.com/zeroaltitude/theseus/issues>.
