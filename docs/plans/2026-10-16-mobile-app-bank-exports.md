# What can each bank's mobile app export as a file? (Moony mobile import research)

Date: 2026-10-16

**Method.** For each bank I searched (WebSearch, standard, then extended when thin; domain-restricted searches worked best) and fetched the bank's own help pages, press releases and App Store listings where reachable. Facts are labelled: **official** (bank's own help/FAQ/press/App Store text, read directly), **official-excerpt** (bank page seen only as a search-tool excerpt because the page itself timed out, returned 403/500, or was a PDF I could not parse), **secondary** (accounting-software guides, blogs, forums, e.g. flatly.cz, iÚčto, FinExpert), **unverified** (nothing found; I list what I looked at). I could not run the apps, so every menu path is from documentation, not from a device. Several official help pages do not say whether a step is in the app or on the web; that is stated per bank. "Share sheet" = the bank documents that the file goes to the iOS/Android share sheet or can be sent on; most banks say only "download", so the default answer is unverified (on a phone a downloaded PDF/CSV normally lands in Files or the share sheet, but that is OS behaviour, not something the banks document).

## Summary table

| Bank | Mobile app exports a file? | Formats in app | Date range? | Share sheet? | CSV only on web? | Status |
|---|---|---|---|---|---|---|
| air-bank | Yes, statements only | PDF | Monthly statements; "mimořádný výpis" on request (period choice not documented) | Unverified ("download") | Yes - payment-history CSV is in internet banking only | verified-official (PDF); CSV-on-web official-excerpt + secondary |
| ceska-sporitelna (George) | Yes, statements; transaction export in app not documented | PDF (statements, secondary for format) | Statements per period; web export has free period + field choice | Yes for statements (official: "stáhněte si ho a rovnou pošlete") | Probably (CSV/XLSX/JSON/MS Money documented for web; app unverified) | verified-official (statement share); CSV in app unverified |
| chase | Yes, statements only | PDF | Statements by month/year (up to 7 years) | Secondary: Share/Download icon | Yes - CSV/QFX/QIF/QBO are desktop (official) | verified-official + secondary |
| csob (Smart) | Not verified | Unverified (App Store listing mentions no export/statements) | Old guide: Pohyby view up to 550 days, no export | Unverified | Likely (CSV/SLK/TXT documented for internet banking) | unverified (app); secondary (web) |
| dkb | Yes, PDF statements in Postfach; transaction-list export documented, app/web not stated | PDF | Statements monthly; list export of filtered Umsatzübersicht | Unverified | Yes per secondary (official page mentions only PDF) | verified-official (PDF) + secondary (CSV) |
| fio (Smartbanking) | Yes, statements | PDF (official page mentions only PDF) | Per statement frequency set by client | iOS press release 2013: "view or share" PDF; current unverified | Yes - CSV/GPC are internetbanking (official FAQ) | verified-official |
| ing-de | Statements as PDF in Post-Box; Umsätze export not documented for app | PDF (Post-Box) | Online-Banking shows up to 390 days | Unverified | Yes (official: "Exportieren" in Online-Banking; business CSV explicitly web-only) | official-excerpt; app export unverified |
| komercni-banka (KB+) | Single-payment PDF only (verified); statements/export unverified | PDF "Potvrzení" per payment | n/a | Yes for the payment confirmation (official: share via available apps) | Probably (MojeBanka/KB+ web has CSV; no app evidence) | partly official; mostly unverified |
| mbank (CZ) | Yes - full history export | PDF, CSV, HTML, ABO | Yes (account, payment types, time period) | Not stated | No - CSV is in the app | verified-official |
| moneta (Smart Banka) | Yes, statements | PDF (official 2018 press release) | Unverified | Unverified | CSV not offered anywhere I found | official but dated; current app unverified |
| n26 | Yes, monthly statements | PDF | Monthly only in app; custom range is web-only | Unverified | Yes - custom-range PDF/CSV report is web-only (official) | verified-official |
| raiffeisenbank | Yes, statements | PDF | Monthly; statement period set in web, not in app; 18 months | Unverified | Probably (official: CSV only from transaction history in internet banking) | verified-official (PDF); CSV in app unverified |
| revolut | Yes | PDF or Excel (personal); CSV only in Business app | Yes, custom timeframe | Official-excerpt: "Export" icon top right | CSV label: personal app shows Excel, not CSV | official-excerpt + secondary |
| sparkasse | PDF statements in Postfach; Umsatz export not documented for app | PDF | App shows 90 days by default, extendable to 445 | Secondary: share/download icon | Yes - CSV/MT940/CAMT are Online-Banking (regional Sparkasse help) | official-excerpt (regional) + secondary |
| uk-debit-credit | Generic UK CSV layout; not a bank. Skipped. | - | - | - | - | n/a |
| wise | Yes | PDF, CSV, XLSX (web adds MT940, QIF, CAMT.053) | Yes, up to 365 days per statement | Unverified | No (CSV in app); only the accounting formats are web-only | verified-official |
| unicredit (no preset) | Statements in "Dokumenty" (format unstated) | Unverified (likely PDF) | Unverified | Unverified | Yes per secondary (CSV from Online Banking "Historie plateb") | secondary/unverified |
| partners-banka (no preset) | Yes - app is the only channel | CSV or PDF (forum) | Yes, "dle zadaných kritérií" (official terms excerpt) | Unverified (can "download to phone") | No web banking exists | official-excerpt + secondary |
| creditas (no preset) | Statements in app; export not documented | Unverified | Unverified | Unverified | Web IB: Pohyby > Export XML/PDF/CSV | unverified (app) |

---

## air-bank (Air Bank, CZ) - "Moje Air" app

- **Menu path (official, current site):** Chytrý přehled -> select the account -> three dots (top right) of the account detail -> **Výpisy z účtu** -> tap a statement -> downloads as PDF.
- **Formats:** PDF in the app. ABO and XML only for business accounts and only in internet banking.
- **Date range:** You export "jakýkoliv již vytvořený výpis" (already generated monthly statements). A "Žádost o mimořádný výpis" button creates one on demand; it arrives in internet banking or by e-mail, usually in minutes, e-mail attachment is password-protected with the primary phone number. Cannot be created for the current day. Whether an arbitrary period can be typed for the extraordinary statement is not stated on the page: unverified.
- **CSV:** The official internet-banking help for payment history says the whole history can be printed or exported to csv (two icons bottom-left of each page); the page does not mention apps. An Air Bank X/Twitter reply (2020) says the CSV export is at the end of the payment list in the "bankovnictví" (web). No source shows CSV in the app. Flatly.cz (2026) states the mobile app "umí jen PDF".
- **Quirks:** Statements are issued only in Czech. The mobile app can set up regular statement delivery (e-mail free, mail 25 Kč); for business accounts that option is not in the app yet.
- **Status:** PDF-in-app verified-official; CSV-web-only is official-excerpt + secondary.
- **Sources:**
  - [Export výpisů](https://www.airbank.cz/co-vas-nejvic-zajima/export-vypisu/)
  - [Výpisy z účtu](https://www.airbank.cz/co-vas-nejvic-zajima/vypisy-z-uctu/)
  - [Historie plateb help (internet banking)](https://ib.airbank.cz/static-html/main/IB_MENU_SIDE_TRANSACTION_HISTORY/help.html)
  - [Air Bank on X, 2020 (via search excerpt)](https://x.com/Air_Bank/status/1234070978176724994)
  - [Flatly: export from all Czech banks](https://flatly.cz/blog/jak-exportovat-transakce-z-banky)

## ceska-sporitelna (Česká spořitelna, George app)

- **Statements in the app (official):** The news page "Novinky v mobilní aplikaci George" (section "Výpisy z účtu vždy po ruce", heading "Listopadové novinky", year not shown) says you can get a statement in George on the phone: "Jednoduše si ho stáhněte a rovnou pošlete, komukoliv chcete" - i.e. download and send onward. Changing statement frequency/address/language is stated for the computer only. A secondary guide (cistepc.cz, 23. 11. 2024, updated 7. 10. 2026) gives the path: open the account -> function menu -> **Výpisy z účtu** / **Výpisy** -> choose period -> download PDF -> save to Files or share. It notes the label may vary by version.
- **Transaction export (CSV etc.):** The official FAQ "Jak z George exportovat transakce?" says: click the arrow icon top right of the transaction history ("Historie"), choose any period and the fields; formats CSV, PDF, MS Excel, JSON, MS Money. The page does not say web vs. app. George Business help (web) lists CSV, XLSX, JSON, and PDF via print, with a period dialog; "ABO" only in Business. The two sources I found that describe a mobile flow cover statements only. Whether the app has the export arrow is **unverified**; forum comments from 2019-2021 say a signed export was found on the computer but not in the app (anecdotal, old).
- **Formats:** PDF statements (secondary for the format); transaction export formats above (web, documented).
- **Date range:** Statements per period; web export has free from-to period and field selection.
- **Quirks:** George Business and personal George differ; ABO/GPC only in Business. The App Store listing of the personal app mentions no export or statements (one user review asks for PDF statement viewing without saving).
- **Sources:**
  - [Novinky v mobilní aplikaci George](https://www.csas.cz/cs/page/george-novinky)
  - [Jak z George exportovat transakce?](https://www.csas.cz/cs/caste-dotazy/jak-z-george-exportovat-transakce)
  - [George Business: Tisk a export historie](https://www.csas.cz/cs/george-help/george-business/transakce-a-vyhledavani/tisk-a-export/tisk-a-export-historie)
  - [Čisté PC: výpis z účtu ČS](https://www.cistepc.cz/jak-ziskat-vypis-z-uctu-ceska-sporitelna/)
  - [Mobile George feature page](https://www.csas.cz/en/mobile-apps/george-go)
  - [George Česko, App Store](https://apps.apple.com/cz/app/george-%C4%8Desko/id1163368801?l=cs)

## chase (Chase, US)

- **Statements in the app (official):** Profile & Settings icon (top right) -> Document manager -> **Statements & documents** -> Statements -> pick account, year, statement; opens as PDF. Up to 7 years depending on account type.
- **CSV/QFX/QIF/QBO:** Chase's own Spending Planner page says the spreadsheet (CSV), Quicken Web Connect (QFX), Quicken/Microsoft Money (QIF) and QuickBooks Web Connect (QBO) downloads are for "desktop"; for tax season it offers a PDF of the last calendar year's transactions. No official page I found describes CSV in the personal mobile app. The exact desktop path ("Download account activity" / download icon above the transaction list) is from secondary blogs only.
- **Date range:** Statements by month. Desktop activity download: secondary sources disagree (90 days vs. up to two years); Chase says range varies by account type. Unverified.
- **Share sheet:** Secondary only ("tap the statement, then Share or Download icon").
- **Quirks:** Third-party blogs flag a new-device verification prompt before downloads on desktop.
- **Status:** statements verified-official; "CSV desktop-only" official but indirect.
- **Sources:**
  - [Paperless statements](https://www.chase.com/personal/mobile-online-banking/login-paperless)
  - [Spending Planner](https://www.chase.com/personal/financial-tools/build/spending-budget)
  - [Statements and documents](https://www.chase.com/personal/mobile-online-banking/statements)
  - [Secondary: invoicedataextraction.com](https://invoicedataextraction.com/blog/chase-bank-statement-to-excel)

## csob (ČSOB, ČSOB Smart app)

- **Result: unverified.** I found no ČSOB page that describes exporting statements or transactions from the Smart app.
- **What I looked at:** ČSOB Smart App Store listing (current, v17.12.2: payment history up to 5 years with search, no mention of statements/export/PDF/CSV); the old "Návod ke službě ČSOB Smartbanking" PDF (search excerpt only: **Přehledy -> Pohyby** list for a chosen period up to 550 days, filters, no export); the ČSOB app brochure (04/2025, readable part covers insurance only); ČSOB online help.
- **Internet banking (official-excerpt + secondary):** Přehledy -> Výpisy -> Elektronické výpisy; "Nastavení výpisů"; statement formats PDF, TXT, SEPA XML, GPC; monthly statements kept 19 months. Transaction history can be exported as CSV, SLK or TXT (iÚčto, secondary; no detailed path). The ČSOB help page itself returned HTTP 500 to my fetch; I read it via search excerpts.
- **Quirks:** "Pohyby" is explicitly not an official statement according to the help excerpt.
- **Sources:**
  - [ČSOB Smart, App Store](https://apps.apple.com/cz/app/%C4%8Dsob-smart/id1528483245?l=cs)
  - [Výpisy - ČSOB nápověda](https://online.csob.cz/web/csob-napoveda/prehledy/vypisy/elektronicke-vypisy)
  - [Návod ke službě ČSOB Smartbanking (old)](https://www.csob.cz/documents/10710/149881/CSOB_Smartbanking.pdf)
  - [Tommystachi: ČSOB výpis (secondary)](https://tommystachi.cz/csob-3/)
  - [iÚčto: ČSOB pohyby (secondary)](https://www.iucto.cz/jak-na-import-bankovnich-pohybu-csob-do-iucta/)

## dkb (DKB, DE)

- **Statements in the app (official):** Profil -> **Postfach**; monthly Girokonto statements after the 5th, credit-card statements after the 22nd, most documents up to 10 years; documents from before the early-2025 Postfach change are under "Archivierte Dokumente". Open, print, download or save locally.
- **Transaction list export (official):** "Du kannst auch einfach eine Liste deiner Umsätze als PDF exportieren. Gehe dazu in der (gefilterten) Umsatzübersicht auf Exportieren." The page does not say app vs. web and names only PDF. No CSV on the official page.
- **CSV (secondary):** Several guides (smartkontoauszug.de, depotstudent.de, antwort.net; search excerpts only, latest ~73 days old) say the DKB app has no CSV/Excel export and that CSV is in desktop web banking (Umsatzseite -> choose period -> export icon top right). Not confirmed by DKB.
- **Share sheet:** Unverified.
- **Status:** PDF verified-official; "CSV web-only" secondary.
- **Sources:**
  - [DKB: Kontoauszüge und Abrechnungen](https://www.dkb.de/fragen-antworten/wie-erhalte-ich-kontoauszuege-und-kartenabrechnungen)
  - [DKB-App page](https://www.dkb.de/privatkunden/girokonto/banking-app)
  - [Secondary: smartkontoauszug.de DKB](https://smartkontoauszug.de/kontoauszug/dkb)
  - [Secondary: depotstudent.de DKB export](https://depotstudent.de/dkb-export-csv-excel/)

## fio (Fio banka, Smartbanking)

- **Statements in the app (official, page dated 1. 8. 2025):** Under the account name three buttons: **Výpisy a potvrzení**, **Zůstatky**, and (most account types) **Zaplať mi**. "Výpisy a potvrzení" holds movement statements "podle nastavení klienta" plus fee/interest statements for mortgage accounts. Only PDF is mentioned. Individual payment confirmations: **Pohyby** -> open a transaction -> download confirmation (PDF).
- **Sharing:** A 2013 Fio press release says Smartbanking for iOS can "zobrazovat nebo sdílet výpisy z účtu ve formátu PDF" (dated, current behaviour unverified).
- **CSV/GPC:** Official FAQ: "Výpis z účtu je možné vytisknout přímo v internetovém bankovnictví, popřípadě vygenerovat ve formátech *.csv a *.gpc" (internetbanking: Přehledy -> Pohyby na účtu, per Flatly). The Smartbanking FAQ has no export/print entry. The official Fio API (token created in web internetbanking) supports XML, MT940, OFX, GPC, CSV, JSON, HTML, PDF - relevant as a non-file alternative, not an app export.
- **Date range:** Statement frequency is whatever the client set (stored setting).
- **Status:** verified-official. Note that the Fio functions page I could open did not itself mention statements; the statement text is from the 1. 8. 2025 news page.
- **Sources:**
  - [Smartbanking: Mějte dokonalý přehled (1. 8. 2025)](https://www.fio.cz/spolecnost-fio/media/aktuality/312138-smartbanking-mejte-dokonaly-prehled)
  - [Fio FAQ internetbanking/Smartbanking](https://www.fio.cz/o-nas/faq/bankovni-sluzby/1-internetbanking-smartbanking)
  - [Press release 2013: výpisy v iOS](https://www.fio.cz/zpravodajstvi/tiskove-zpravy/131129-fio-smartbanking-nabizi-sablony-a-vypisy-jiz-i-pro-ios)
  - [Fio API Bankovnictví (PDF)](https://www.fio.cz/docs/cz/API_Bankovnictvi.pdf)
  - [Fio Smartbanking CZ, App Store](https://apps.apple.com/cz/app/fio-smartbanking-cz/id1451236597?l=cs)

## ing-de (ING Germany)

- **App:** Monthly statements are PDFs in the **Post-Box**, reached in the app via **Service** in the bottom menu (ing.de help, search excerpt). That is the only app file I could confirm.
- **Umsätze export (official-excerpt):** ING's help describes it only for Online-Banking: "Klicken Sie im Online-Banking am Seitenende auf Exportieren" in the Umsatzanzeige; displayed transactions go back up to 390 days. For Geschäftskonto the FAQ says PDF/CSV ("Umsatzübersicht erstellen") with CSV "nur in der Web-Version verfügbar". No official page found documents an Umsatz export in the private-customer app. Secondary sources conflict (one says the app only shows/scrolls, one says desktop-only, one says possible via the Umsatzliste but vague).
- **Quirks:** ING's pages timed out for direct fetch; I rely on search excerpts of ing.de pages (the tool noted some are 1,100-2,750 days old). CSV from the web is semicolon-separated UTF-8 per a secondary guide.
- **Status:** official-excerpt for web; app export unverified.
- **Sources:**
  - [ING Hilfe: So läuft Banking bei uns](https://www.ing.de/hilfe/banking/)
  - [ING Geschäftskonto FAQ](https://www.ing.de/business/geschaeftskonto/faq/)
  - [ING Girokonto-Features (app)](https://www.ing.de/hilfe/app/girokonto-features/)
  - [Secondary: kontocsv.de ING (Jan 2026)](https://www.kontocsv.de/ratgeber/ing-kontoauszug-csv)

## komercni-banka (Komerční banka, KB+ app)

- **Verified in KB+ (official):** In the payment detail, a **Potvrzení** button at the bottom creates a PDF confirmation of one payment, which in KB+ can be shared through available apps ("V KB+ můžete potvrzení sdílet prostřednictvím dostupných aplikací"); in internet banking the PDF downloads to the computer.
- **Statements in KB+:** KB's "Potřebuji výpis k účtu?" says statements can be sent electronically for free at the start of each month, but gives no app path. The KB Dokumenty page mentions "Zobrazit dokumenty" for KB+ (search excerpt). Whether KB+ has a statement list/export: **unverified**.
- **CSV:** Flatly.cz (updated 15. 10. 2026) says: MojeBanka -> **Stažení účetních dat** -> "kompatibilní média (GPC)"; "případně v KB+ výpis v csv. formátu"; one file or per day; data for the last 730 days. No KB+ menu path, no app-vs-web distinction. KB publishes a client CSV format spec (MojeBanka/Business) and the KM/BEST formats for web. The official KB+ "STATDA" API is business-only.
- **Status:** payment confirmation verified-official; statements/export unverified.
- **Sources:**
  - [Potvrzení o platbě v KB+](https://www.kb.cz/cs/podpora/ucty-a-platby/jak-ziskat-potvrzeni-o-platbe-v-kb)
  - [Potřebuji výpis k účtu?](https://www.kb.cz/cs/podpora/ucty-a-platby/potrebuji-vypis-k-uctu)
  - [KB Dokumenty](https://www.kb.cz/cs/o-bance/dokumenty)
  - [MojeBanka: Stažení účetních dat (legacy web)](https://mojebanka.kb.cz/help_cz/a06.htm)
  - [Klientský formát CSV KB](https://mojebanka.kb.cz/file/cs/format_CSV_KB.pdf)
  - [Flatly: export transakcí](https://flatly.cz/blog/jak-exportovat-transakce-z-banky)

## mbank (mBank, CZ/PL)

- **CZ app (official blog, 8. 9. 2023):** log in -> bottom menu **finance** -> top bar **historie** -> blue arrow icon (top right) -> choose account, payment types (incoming, outgoing, card, interest credits...) and time period -> download as **PDF, CSV, HTML or ABO**. This is the only bank among the 19 where the official page documents CSV and ABO in the mobile app with a free period.
- **Web (CZ):** Platby -> Historie -> Seznam operací -> "Změnit zobrazení" -> account, date range, filters -> "Formát dat" (HTML similar to the e-mailed statement, PDF with bank header). Also **FINANCE -> HISTORIE -> Výpis operací**.
- **Share sheet:** Not stated on the pages I read. The page is dated 2023; menus may have moved.
- **Quirks:** Monthly statements also come by e-mail by the 2nd of the next month (optionally encrypted PDF). mBank added ABO and reworked CSV so other software can import it.
- **mBank Poland:** Official FAQ: Finanse > Historia > Zestawienie operacji; formats PDF, HTML, CSV; account, operation type, date range; history moved into "Finanse" in app and web (article 16.07.2024, updated 3.09.2026). The PL FAQ does not say whether the export is in the app; a 2019 Polish article (secondary, search excerpt) says the app's history had an "Eksportuj listę" option. PL app status: unverified.
- **Sources:**
  - [mBank CZ: Jak získat výpis](https://www.mbank.cz/blog/post,972,jak-ziskat-vypis-z-bankovniho-uctu.html)
  - [mBank CZ: Generování výpisů](https://www.mbank.cz/blog/post,747,generovani-vypisu.html)
  - [mBank PL: Historia w serwisie transakcyjnym](https://www.mbank.pl/indywidualny/aplikacja-i-serwis/pytania-i-odpowiedzi-serwis/historia-w-serwisie-transakcyjnym/)
  - [mBank PL: Zmieniamy lokalizację historii](https://www.mbank.pl/artykuly/zmieniamy-lokalizacje-historii-transakcji/)

## moneta (MONETA Money Bank, Smart Banka)

- **Statements in the app:** Official FAQ: electronic statements are free, stored in Internet Banka, "najdete je i ve Smart Bance". Official press release of **3. 5. 2018**: the new Smart Banka lets you "na jedno kliknutí si stáhnout výpis z běžného účtu ve formátu pdf". The current App Store listing (v96.1.0) and the Smart Banka feature page mention no statements/export/PDF/CSV; so the PDF feature is documented only by the 2018 release and the FAQ. Menu path in the app: unverified.
- **CSV:** None found. Internet Banka offers statements as PDF, TXT, XML (official guide, search excerpt); Flatly describes an ABO/GPC export for accountants configured under Osobní nastavení -> Nastavení pro účetní systémy (web).
- **Date range, share sheet:** unverified.
- **Status:** official but dated; current app behaviour unverified.
- **Sources:**
  - [Tisková zpráva 3. 5. 2018](https://www.moneta.cz/servis-pro-media/tiskove-zpravy/detail/klienti-moneta-money-bank-uvidi-sve-ucty-u-jinych-bank)
  - [FAQ: Jak získat výpis zdarma](https://www.moneta.cz/caste-dotazy/odpoved/jak-ziskat-vypis-z-uctu-zdarma)
  - [FAQ: Kde najdu výpisy](https://www.moneta.cz/caste-dotazy/odpoved/kde-najdu-vypisy-z-uctu-a-potvrzeni-o-zaplacenych-urocich-z-hypoteky-nebo-uveru-ze-stavebniho-sporeni-)
  - [MONETA Smart Banka, App Store](https://apps.apple.com/app/id1122443334)
  - [Smart Banka page](https://www.moneta.cz/smart-banka)

## n26 (N26, DE)

- **App (official):** Finances tab (DE: Finanzen) -> choose account or Space -> gear icon -> **Balance statements** (Kontoauszüge) -> pick a month. PDF only, monthly.
- **Custom range / CSV (official):** "Account Activity" reports - start/end date, PDF or CSV - are described only for the web app: Home tab -> account/Space -> **Downloads** -> Account Activity (alt.: Spaces tab -> Settings -> Balance Statements). The German page's own summary table shows these as not described for the app.
- **Share sheet:** Unverified.
- **Quirks:** Statements are valid without signature/stamp. GDPR personal-data export is a separate JSON feature, not a statement.
- **Status:** verified-official.
- **Sources:**
  - [N26 DE: Kontoauszug herunterladen](https://support.n26.com/de-de/konto-und-personliche-informationen/kontoauszuge-und-kontobestatigungen/kontoauszug-herunterladen)
  - [N26: export a list of my transactions](https://support.n26.com/en-it/fixing-an-issue/payments-and-transfers/how-to-export-a-list-of-my-transactions)
  - [N26: How to get a bank statement](https://support.n26.com/en-eu/payments-transfers-and-withdrawals/balance-and-limits/how-to-get-bank-statement-n26)

## raiffeisenbank (Raiffeisenbank CZ)

- **App statements (official EN FAQ):** main page -> tap the account -> **Detail** tab -> **Statements** tab -> select statement -> **Download statement**. Format PDF. Statement fee statements: Profil (folder icon) -> Dokumenty. The statement period can be changed only in internet banking (Výpisy a potvrzení -> Nastavení výpisů), not in the app. Internet banking keeps monthly statements for 18 months.
- **CSV (official, Equa-migration FAQ):** statements are downloadable "ve formátech PDF, CSV (pouze z transakční historie), ABO, Gemini" from the new internet banking; steps for CSV not given and mobile availability not stated. Secondary guides describe web transaction history -> export CSV, file named `Pohyby_<číslo účtu>_<datum>.csv`, max 20,000 rows per file. ABO/GPC is a paid service activated at a branch. App CSV: unverified.
- **Share sheet, free date range in app:** unverified.
- **Sources:**
  - [RB: Výpisy a potvrzení k účtu (FAQ)](https://www.rb.cz/en/podpora/nastaveni-uctu/vypisy-a-potvrzeni-kuctu)
  - [RB: Převedení Equabank (formats)](https://www.rb.cz/spojeni-equa/osobni-copy)
  - [RB: Personal accounts FAQ](https://www.rb.cz/en/personal/accounts/services-to-account/internet-banking/faq/accounts)
  - [RB: Mobile banking](https://www.rb.cz/en/personal/accounts/services-to-account/mobile-banking)

## revolut (Revolut)

- **Personal app (official-excerpt):** Home -> Accounts -> select currency -> **More (...)** -> **Statement** -> choose timeframe -> document type **PDF or Excel** -> Generate; the Pockets statement help says the generated file is saved with the **Export** icon (top right), which is the share/save entry. Cannot start before the account open date. Statements are per currency account.
- **CSV:** Official Revolut **Business** pages list PDF and CSV for transaction statements and PDF, CSV, XML (CAMT.053), TXT (MT940) for account statements; personal pages list PDF/Excel. Third-party guides are inconsistent: some say personal "Excel" downloads a CSV, some list CSV directly (vestelonflow.com, June 2026, lists only PDF and Excel). Treat personal CSV as unconfirmed; check the downloaded file type.
- **Quirks:** help.revolut.com returned HTTP 403 to fetch, so the official statements above come from search excerpts of the help pages.
- **Sources:**
  - [Personal: statement per currency](https://help.revolut.com/help/profile-and-plan/managing-my-account/account-statement-per-chosen-currency/)
  - [Pockets statement (Export icon)](https://help.revolut.com/en-DE/help/profile-and-plan/managing-my-account/get-a-pocket-s-statement/)
  - [Business: transaction statements](https://help.revolut.com/business/help/managing-my-business/viewing-my-account-statements/finding-my-account-statement/)
  - [Secondary: vestelonflow.com](https://www.vestelonflow.com/blog/export-revolut-statement)
  - [Secondary: kleev.ai](https://kleev.ai/blog/export-revolut-csv)

## sparkasse (Sparkasse, DE)

Sparkassen are regional institutes; wording differs per institute. I read two regional Sparkasse pages.

- **App (official regional):** **Profil -> Postfach** for Kontoauszüge (PDF); the Postfach must be enabled in Online-Banking (Einstellungen - Empfangs- und Versandregeln). Secondary: a download/share icon next to the statement.
- **Umsatz export (official regional):** Online-Banking: choose account and period -> Aktualisieren -> **Export** -> CSV, MT940 or CAMT ("Exportierte Umsätze im CSV-Format können Sie in Excel einlesen"; MT940 domestic, CAMT SEPA). The page describes no Umsatz export in the app. Secondary (antwort.net, 2020): app export not possible; some Sparkassen offer per-transaction PDF/image.
- **Date range:** App shows only the last 90 days by default (PSD2); can be extended to 445 days (iOS: "Umsätze abholen ab", Android: "Aktualisieren ab Datum").
- **Quirks:** Sparkassen moved to camt V08 and no longer deliver MT940/MT942 on some EBICS paths; CSV amounts may carry minus signs and column layouts differ per institute (secondary).
- **Sources:**
  - [Sparkasse Westmünsterland: Tipps und Hilfen](https://www.sparkasse-westmuensterland.de/fi/home/services-und-kontakt/online-banking/tipps_und_hilfen.html)
  - [Berliner Sparkasse: Kontoauszüge](https://www.berliner-sparkasse.de/de/home/service/kontoauszuege.html)
  - [Sparkasse: Elektronisches Postfach](https://www.sparkasse.de/pk/produkte/konten-und-karten/banking/online-services/elektronisches-postfach.html)
  - [Secondary: antwort.net Sparkasse CSV](https://www.antwort.net/a/technologie/wie-kann-ich-in-der-sparkasse-app-einen-csv-export-meiner-transaktionen-durchfuehren.html)

## uk-debit-credit

Generic UK CSV layout, not a bank; no app to research. Skipped.

## wise (Wise)

- **App (official, DE + EN help):** open a currency or jar -> three dots (top right) -> **Statements and reports** (DE: Kontoauszüge und Berichte) -> choose document type, enter Wise password. From home: initials -> Statements and reports -> Statements -> "Create a statement" (currencies/jars, period, format, language). Bulk multi-currency download: web and Android, not iOS.
- **Formats:** **App: PDF, CSV, XLSX. Web: PDF, XLSX, CSV, MT940, QIF, CAMT.053** (CAMT.053 only "version 10"). Wise's API also serves JSON, CSV, PDF, XLSX, CAMT.053, MT940, QIF.
- **Date range:** Free period, max 365 days per statement; longer needs several downloads.
- **Quirks:** Statements cover Wise currency-account transactions only; a transfer list is on the Transactions page. Optional "Stamp and sign" PDF. Viewing statements requires SCA/password. Share sheet: unverified.
- **Status:** verified-official.
- **Sources:**
  - [Wise DE: Kontoauszug herunterladen](https://wise.com/de/help/articles/2736049/wie-lade-ich-einen-kontoauszug-herunter)
  - [Wise EN: How do I download a statement](https://wise.com/help/articles/2736049/how-do-i-download-a-statement)
  - [Wise API: Balance statements](https://docs.wise.com/api-reference/balance-statement)

## unicredit (UniCredit Bank CZ, Smart Banking) - no preset

- **App (official App Store listing):** "Výpisy z účtu, Výpisy ke kreditní kartě a další dokumenty najdete v sekci **Dokumenty**." Format, period choice and sharing are not stated. No export or CSV mentioned in listing or release notes.
- **CSV:** Web Online Banking: account -> **Historie plateb** -> Vyhledat (top right) -> export search result to .csv; single transactions as PDF confirmation (UniCredit user guide, ~2,100 days old, search excerpt). Flatly.cz (2026): UniCredit retail offers only CSV, no ABO/GPC. A search tool summary of a 2019-2020 UniCredit "services and parameters" PDF suggested CSV export is listed for mobile banking; I could not open the PDF (HTTP 403), so treat as an unconfirmed lead.
- **Status:** secondary/unverified.
- **Sources:**
  - [Smart Banking, App Store](https://apps.apple.com/cz/app/smart-banking/id468057018?l=cs)
  - [UniCredit Online Banking guide](https://www.unicreditbank.cz/en/online-banking-tutorial/pruvodce-novym-ob.html)
  - [Flatly](https://flatly.cz/blog/jak-exportovat-transakce-z-banky)

## partners-banka (Partners Banka) - no preset

- **App is the only channel** (no web internetbanking). Official terms (search excerpt, not read in full; the PDF is compressed): the client can "kdykoliv a dle vašich parametrů vygenerovat a uložit výpis z jakéhokoliv účtu" in the mobile app; the generated statement stays in the app 30 days and can be downloaded to the phone.
- **Formats/path (secondary, FinExpert post 16. 3. 2024):** in transaction history in the app: **Více -> Dokumenty -> Generovat výpis** -> **CSV** or **PDF**. One user also exports CSV from "internet banking" for Excel (conflicts with the no-web claim; date unknown).
- **Share sheet:** unverified. **Date range:** by criteria entered (official excerpt).
- **Status:** official-excerpt + one dated forum post.
- **Sources:**
  - [Obchodní podmínky pro platební styk (11/2023)](https://www.partnersbanka.cz/userfiles/obchodni-podminky-pro-platebni-styk-ucinne-k-1-11-2023-2778659f.pdf)
  - [Podmínky mobilní aplikace (3/2024)](https://www.partnersbanka.cz/userfiles/podminky-mobilni-aplikace-ucinne-k-4-3-2024-6d45bd0e.pdf)
  - [FinExpert forum](https://forum.finexpert.e15.cz/viewtopic.php?p=13694248)

## creditas (Banka CREDITAS, Creditas One app) - no preset

- **App:** App Store listing (current): statements and templates are "bezpečně uložené a kdykoliv dostupné"; no export, PDF, CSV, or sharing mentioned. App path/format: **unverified**.
- **Web internetbanking (official-excerpt):** monthly statements: account detail -> **Další volby -> Výpisy k účtu**; custom period: **Pohyby na účtu -> Export** (bottom right), formats **XML, PDF, CSV**; API docs list PDF and ABO(GPC) for statements and camt.053 XML/CSV for history. The Creditas manual PDF returned 403 so I used search excerpts. Terms say the app lives inside the internet-banking agreement, so it may share functions, but nothing confirms it.
- **Sources:**
  - [CREDITAS One, App Store](https://apps.apple.com/cz/app/creditas-one/id6738919456)
  - [Internetové bankovnictví Creditas](https://www2020.creditas.cz/internetove-bankovnictvi)
  - [Otevřené bankovnictví a Creditas API - manuál](https://www.creditas.cz/files/otevrene-bankovnictvi-a-creditas-api-manual.pdf)

---

## Implications

- **Phone-friendly (structured data straight from the app):** **mBank CZ** (CSV + ABO + PDF + HTML, free period, official), **Wise** (CSV/XLSX/PDF, up to 365 days per file; web adds MT940/QIF/CAMT), **Revolut** (Excel/PDF, free timeframe; CSV only in Business, so Moony's Revolut preset needs the personal Excel/CSV file checked), and probably **Partners Banka** (CSV/PDF in app, one forum post). These need no web detour.
- **PDF-only from the app:** Air Bank, Fio, Raiffeisenbank, MONETA (dated), ČS George (statements), N26 (monthly), Chase, DKB (Postfach), ING (Post-Box), Sparkasse (Postfach), KB+ (single-payment PDF), UniCredit (probably). For these, an on-phone import means either PDF statement parsing or a fallback to the bank's web banking.
- **CSV/structured data needs the web (or API):** N26 custom-range CSV, Chase CSV/QFX/QIF/QBO (desktop), DKB CSV, ING CSV, Sparkasse CSV/MT940/CAMT, Fio CSV/GPC (plus Fio API token), ČSOB CSV/SLK/TXT, KB CSV, Raiffeisenbank CSV, Air Bank CSV, UniCredit CSV, Creditas CSV/XML. Whether a bank's web banking works in mobile Safari/Chrome and lets you save the CSV to Files was not tested here.
- **Pattern:** neobanks and mBank (Wise, Revolut, mBank) give data exports in-app; large CZ and DE incumbents (ČS, ČSOB, KB, Raiffeisen, Moneta, Air Bank, ING, DKB, Sparkasse, Chase) give PDF statements in the app and keep CSV/accounting formats (ABO/GPC, MT940, CAMT) on the web. N26 is the neobank exception (custom CSV is web-only).
- **Biggest open question: ČS George.** Statements are officially downloadable and shareable on the phone; the transaction export (CSV/Excel with a free period) is documented but not tied to the app. It is the most valuable bank to verify on a real device, along with KB+ and ČSOB Smart (nothing found).
- **Share sheet is rarely documented.** Only George (statements) and the 2013 Fio iOS note explicitly mention sending/sharing; Revolut's Export icon is the likely share entry. Treat share-sheet support as a per-bank device test, not a documented guarantee.
- **Date-range limits to design for:** statements are per month in Air Bank, N26 (app), Raiffeisenbank, DKB, ING, Sparkasse, Fio (per set frequency); free periods exist in mBank, Wise (365 days), Revolut, Partners Banka, and web exports. Sparkasse app history is 90 days by default (445 max).
- **Accuracy caveats:** Fio, ČSOB Smart, KB+, Moneta, UniCredit, Creditas, ING-app and Sparkasse-app conclusions rest on absence of evidence plus web-only documentation; none of these should be hard-coded as "app cannot export" without a device check.
