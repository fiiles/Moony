# First-run welcome and onboarding polish

Approved in chat on 2026-10-12. This is a frozen design snapshot.

First run opens with a calm, 2.6-second brand animation: a light ring resolves into
the Moony moon, followed by a welcome and a short personal-finance tagline. The
intro can be skipped and automatically yields to the existing five-step wizard.
Reduced motion skips the intro immediately. Returning to earlier wizard steps
does not replay it; the lock screen does not show it.

Keep the existing warm surfaces, monochrome brand, local Inter font, and semantic
tokens. Reserve a normal-flow header for language selection in both intro and
wizard. Remove the unexplained area count from the preview, call step four “What
to track”, and keep the preview below the form on narrow windows. Currency fields
use the same paper surface, height, border, type, chevron, and focus states as
Select fields, retaining searchable currency menus.

Closing the last step's CSV import dialog must not finish onboarding. Account
creation is retained, the saved fields become read-only, and importing can be
reopened for that same account without creating a duplicate. A separate explicit
action enters the application. This applies to cancellation and successful import
alike; CSV import itself keeps its current behavior.

All new copy ships in all ten locales. No wire contract, database, or backend
changes. Verify frontend gates, Rust gates required by repository policy, and
visually inspect intro, wizard, matching fields, narrow layout, and the last-step
import cancellation/reopening flow with isolated preview data.
