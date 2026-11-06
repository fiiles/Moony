# Mobilní Moony: nezávislý UX audit a nesoulad s desktopem (2026-11-06)

Stav: `main` @ b98c4502 (v1.3.0 + Windows fix). Audit jen čte, v kódu se nic neměnilo.

## Rozsah a metoda

- **Kód.** Celý `src/mobile` jsem porovnal s desktopem ve čtyřech oblastech: Přehled se shellem a nastavením, Finance, Cashflow s rozpočty a Majetek. K tomu přibyla průřezová kontrola vzorů, formátování, textů a přístupnosti. Nejzávažnější tvrzení jsem ověřil přímo v kódu.
- **Živý průchod.** Telefonní shell (`VITE_MOONY_SHELL=mobile`) běžel nad kopií demo profilu (Alex) v okně 390 × 844. Prošel jsem všechny čtyři záložky, detail účtu, transakce, pozice a Nastavení.
  - Platformní vrstvu iOS (WKWebView, klávesnice, Face ID, oznámení) tento průchod neověřuje.
  - Co vyžaduje zařízení, je v poslední sekci.
- **Co není nálezem.**
  - Vědomé vynechávky ze specifikace (`docs/specs/2026-10-16-mobile-app-design.md` §2 a §7): analýza akcií, projekce, plánování cashflow, kalkulačky, editor pravidel, MCP a updater.
  - Skrývání částek, které vlastník zrušil.
  - Výdajové plány, které řeší paralelní worktree.

Závažnost: **V** = vysoká (blokuje úkol nebo vede ke špatné informaci), **S** = střední, **N** = nízká.

---

## Shrnutí: deset věcí, na kterých nejvíc záleží

1. **V: Kategorizace se na telefonu nedá opravit.** Telefon pravidla vytváří („Zapamatovat pro tento účet“, „Zařadit i dalších N plateb“), ale nedá se v něm vypnout ani smazat pravidlo, zapomenout naučeného příjemce ani upravit nebo smazat kategorii. Spec §7 a §8.9 přitom seznam pravidel s vypnutím a smazáním slibuje. → B1
2. **V: Nejde přidat ruční transakci a nejde vrátit import.** Hotovostní účet nebo banku bez exportu nejde vést jinak než zůstatky. Špatně naimportovaný výpis nejde vzít zpět. Spec obojí slibuje (§8.4, §8.6). → A1, A2
3. **V: Přijaté dividendy jsou neviditelné, ale započtené.** Detail ukazuje „Realizovaný výnos + 2 693 Kč · zatím žádný prodej“: číslo obsahuje dividendy, text tvrdí opak. Dividendu nejde zapsat ani zobrazit. → A3 (viděno naživo)
4. **V: Cashflow drží říjnový model, desktop se mezitím změnil.**
   - Kategorie s limitem bez útraty zmizí.
   - Chybí stavy „Limit vyčerpán“ a „Čerpání nelze vyhodnotit“, zbývající částka i poznámka „Jen část období“.
   - Selhání rozpočtů se tiše spolkne.
   - → B2
5. **V: Záznam události jde přes desktopové formuláře.** Nákup, prodej, nová pojistka, účet, úvěr a ruční cena jsou desktopové dialogy. Mají `type="number"` (77 polí v `src/components`) a desktopové velikosti písma. `AmountField` přitom vznikl právě proto, že `type="number"` ve WKWebView odmítá čárku. → C1 (dopad ověřit na zařízení)
6. **S: Připomínky úvěrů na telefonu chybí, přepínač pro ně v Nastavení zůstal.** Jde o „Zůstatek nezadán od …“ a „Fixace skončila …“, přepínač se jmenuje „Zůstatky a sazby úvěrů“. → B3 (ověřeno v kódu)
7. **S: Po zamčení na pozadí se ztratí místo.** Po odemčení aplikace vždy skočí na Přehled. To platí i po minutě na pozadí (výchozí zpoždění zámku), například při kopírování čísla účtu. Přepnutí záložky také zahodí detail, filtr i posun. → C3
8. **S: Oznámení nikam nevedou.** Klepnutí na „Je čas naimportovat výpisy“ neotevře frontu ani import. Spec §6 a §9 slibují, že oznámení i zkratky otevřou cíl přímo. → C4
9. **S: Gesta neplatí obecně a chovají se nepředvídatelně.**
   - Hlavní řádky (účty, úvěry, pojistky, pozice, nemovitosti) nemají swipe ani dlouhý stisk, ačkoli to princip 2 vyžaduje.
   - Swipe doleva znamená na každé obrazovce něco jiného: Kategorie, Nechat (bez vrácení), Skrýt, Smazat.
   - → C2
10. **S: Chyba načtení vypadá jako „nenalezeno“ nebo jako prázdný stav.** Finance při chybě ukáže „Zatím nemáte žádné bankovní účty“, detaily „Pozice nenalezena“, a nikde není „Zkusit znovu“. → C6

---

## A. Chybí oproti desktopu (a §7 to neospravedlňuje)

| # | Záv. | Nález | Důkaz | Doporučení |
|---|---|---|---|---|
| A1 | V | **Ruční transakce nejde přidat.** `···` účtu nabízí jen Upravit účet, Historie zůstatku, Historie importů a Smazat účet. Spec §8.4 a §8.6 slibuje „Přidat transakci“. | [AccountDetail.tsx:237](../../src/mobile/pages/finance/AccountDetail.tsx:237); desktop [BankAccountDetail.tsx:797](../../src/pages/BankAccountDetail.tsx:797), `AddTransactionModal` | Přidat položku do `···`. Desktopový modal se v telefonním shellu sám vykreslí jako sheet, ideálně ale `AmountField` (viz C1). |
| A2 | V | **Import nejde vrátit.** Historie importů je jen ke čtení („undoing an import stays on the desktop“). Spec §8.6 říká „Undo stays in Historie importů“. | [ImportHistorySheet.tsx:17](../../src/mobile/sheets/ImportHistorySheet.tsx:17); desktop `ImportHistoryDialog.tsx:97` | Přidat „Vrátit import“ s ConfirmSheet a desktopovými texty `importHistory.undo*`. |
| A3 | V | **Přijaté dividendy.** Nejde je zobrazit ani zapsat. Ledger pozice má jen Vše, Nákupy a Prodeje. „Realizovaný výnos“ je zobrazený bez poznámky „z toho dividendy X“, kterou desktop má. Ruční výplatu u tickeru bez dat z Yahoo nejde zadat. | [PositionStats.tsx:121](../../src/mobile/components/wealth/PositionStats.tsx:121) vs. [StockDetail.tsx:663](../../src/pages/StockDetail.tsx:663); [position-series.ts:34](../../src/mobile/lib/position-series.ts:34) | Přidat poznámku `detail.realizedDividends`, filtr „Dividendy“ s řádky, „Zapsat dividendu“ přes `StockDividendModal` a invalidaci `['stock-dividends']`. |
| A4 | V | **Kategorie a pravidla.** Viz B1. | | |
| A5 | V | **„Co sledovat“ nejde po prvním spuštění změnit.** Text tvrdí: „Změnit to můžete kdykoli v Nastavení.“ Nastavení ale nemá Viditelnost tříd ani „vyloučit vlastní bydlení z přehledu“. Kdo vypne investice, nevrátí je bez desktopu. | [AreasStep.tsx:11](../../src/mobile/pages/first-run/AreasStep.tsx:11), [routes.ts:62](../../src/mobile/shell/routes.ts:62); desktop `SettingsSectionPages.tsx:20` | Přidat obě sekce (spec §8.9 je uvádí), nebo opravit text. |
| A6 | S | **Zbytek Nastavení podle §8.9.** Chybí export všech dat (JSON), země v profilu (řídí pořadí bankovních formátů a kódování importu), kontrola integrity databáze a stav šifrovaných příloh. | [BackupSection.tsx](../../src/mobile/pages/settings/BackupSection.tsx); desktop `DataSection.tsx:73` | Doplnit, nebo vyškrtnout ze specifikace. |
| A7 | S | **Historie zůstatku je jen ke čtení.** Záznam se špatným datem nejde smazat, a přitom dál ovlivňuje hero, „Změnu letos“ i průměr. | [BalanceHistorySheet.tsx:19](../../src/mobile/sheets/BalanceHistorySheet.tsx:19); desktop `BalanceHistoryDialog` | „Smazat záznam“ přes swipe nebo menu s potvrzením. |
| A8 | S | **V `···` účtu chybí automatická kategorizace a úročení.** Hromadné „použít všechny shody pravidel“ existuje jen v kroku importu. | `use-auto-categorize.ts` (používá ho jen DoneStep); desktop [BankAccountDetail.tsx:507](../../src/pages/BankAccountDetail.tsx:507) | Přidat obě položky (spec §8.4 je má). |
| A9 | S | **Kryptoměny bez zdroje kurzu jsou slepá ulička.** Řádek ukazuje jen „—“. Desktopový banner „N mincí nemá zdroj kurzu … Vybrat záznam“ na telefonu chybí, oprava se dá najít jen v `···` → „Zdroj kurzu“. | `src/mobile/pages/Crypto.tsx`; desktop [Crypto.tsx:276](../../src/pages/Crypto.tsx:276) | Přidat banner (klíče `crypto:unpricedBanner.*` existují). |
| A10 | S | **„Zhodnocení letos“ (TWR/MWR) bez kvalitativních poznámek.** Telefon ignoruje `unpricedTickers`, `dividendGapTickers`, `suspectTradeTickers` a `fxQuality`, takže číslo vypadá autoritativně i tam, kde ho desktop varováním zpochybňuje. | `use-stock-positions.ts:112`; desktop `PerformanceCard.tsx` | Jedna `InlineAlert` pod statistikou s desktopovými texty. |
| A11 | S | **Splity.** Telefon umí jen čekající rozhodnutí. Chybí „Zapsat split“ a seznam provedených splitů, takže když Yahoo split mine, uživatel jen s telefonem ho neopraví. | [SplitAlerts.tsx:18](../../src/mobile/components/wealth/SplitAlerts.tsx:18); desktop `StockDetail.tsx:559` | Zabalit `StockSplitsDialog` do `PositionMenu`, nebo to zapsat do §7. |
| A12 | S | **Chybí karta Začínáme.** Prázdný Přehled nabízí jen „Přidat bankovní účet…“ a vede na prázdné Finance, takže k přidání jsou potřeba dvě klepnutí. | [Overview.tsx:361](../../src/mobile/pages/Overview.tsx:361); desktop `GettingStartedCard` | Přidat kartu podle §8.3 bod 7, nebo otevřít přidání účtu přímo. |
| A13 | N | **Graf sledované akcie nemá značky nákupů a prodejů** („Nákupy a prodeje“, commit 03de60b3). | `WatchDetail.tsx`; desktop `StockMonitorDetail.tsx:499` | Sdílené `@/utils/trade-events`. |
| A14 | N | **Poznámky k nemovitosti nejdou upravit přímo na místě** (desktop RED-06). Komentář v kódu je zastaralý. | [PropertyDetail.tsx:205](../../src/mobile/pages/PropertyDetail.tsx:205) | `updateNotes` s editorem jako `WatchNotes`. |
| A15 | N | **Výnos ostatního aktiva** (`yieldType`/`yieldValue`) se nikde nezobrazuje, jen ve formuláři. | `OtherAssetDetail.tsx` | Statistika nebo podřádek. |
| A16 | N | **Cashflow bez vysvětlujících řádků.** Chybí věty „Nepočítá se do příjmů ani výdajů“, „Celkový pohyb peněz na účtech …“ a „Nezařazené kredity se počítají jako příjem“. Částky převodů jsou bez znaménka. | [Cashflow.tsx:399](../../src/mobile/pages/Cashflow.tsx:399); desktop [Cashflow.tsx:581](../../src/pages/Cashflow.tsx:581) | Doplnit. |

## B. Stejný pojem, jiné chování nebo čísla

**B1 (V): Kategorizace je jednosměrná.**
- `SETTINGS_SECTIONS` obsahuje jen profil, zabezpečení, připomínky, oznámení, zálohu a import ([routes.ts:62](../../src/mobile/shell/routes.ts:62)).
- Picker přitom říká „Zařazuje vlastní pravidlo „X“ → Y“ a nabízí „Změnit kategorii tohoto pravidla“. Chybí ale „Spravovat pravidla“ i jakýkoli seznam pravidel.
- Balíček pravidel se při prvním spuštění zapne potichu ([use-first-run.ts:177](../../src/mobile/hooks/use-first-run.ts:177)) a pak už jen v kroku importu.
- Kategorie jde v pickeru jen založit, nikdy přejmenovat ani smazat s přeřazením.
- Doporučení: stránka „Kategorizace“ (balíčky, seznam pravidel se zapnutím a smazáním, naučení příjemci se „Zapomenout“) a stránka „Kategorie“ s desktopovým přeřazením při mazání.

**B2 (V): Cashflow a limity se rozešly s desktopem po sloučení 2026-10-28.** (Podrobnosti dodal agent pro cashflow, ověřeno podle čísel řádků.)
- **Kategorie jen s limitem nemají řádek.** Mobil staví řádky pouze z `actuals.expenseCategories`. Období bez transakcí, ale s limity, proto spadne do prázdného stavu. Zmizí i kategorie, jejímž transakcím chybí kurz.
  - Mobil: [Cashflow.tsx:264](../../src/mobile/pages/Cashflow.tsx:264), [Cashflow.tsx:300](../../src/mobile/pages/Cashflow.tsx:300).
  - Desktop: `src/utils/cashflow-budget-rows.ts:122`.
- **Hodnocení limitu je jen podmnožina desktopového.**
  - Telefon zná stavy ok, blíží se a nad limitem. Chybí „Limit vyčerpán“, „Čerpání nelze vyhodnotit“, Zbývá a „Jen část období“.
  - Útrata rovná limitu se ukáže jako „blíží se limitu“.
  - Porovnává se po zaokrouhlení na celé jednotky, takže 3 000,40 / 3 000 se ukáže jako „3 000 / 3 000 Kč · nad limitem“.
  - Mobil: [cashflow-rows.ts:72](../../src/mobile/lib/cashflow-rows.ts:72), `CategoryRow.tsx:61`.
  - Desktop: `ExpensesByCategoryCard.tsx:125`.
- **Selhání rozpočtů se nezobrazí.** Limity tiše zmizí a „Nastavit limit“ je neaktivní bez vysvětlení ([use-cashflow.ts:150](../../src/mobile/hooks/use-cashflow.ts:150)).
- **Dva nezávislé dotazy.** Při změně období se na okamžik spojí skutečnost nového období s limity starého. Desktopová specifikace výslovně chce jeden dotaz.
- Doporučení: sdílet `expenseRows` a `limitState` z `src/utils` a mít jeden dotaz. Tím se vyřeší všechny čtyři body najednou.

**B3 (S): Připomínky úvěrů se na telefonu nezobrazí.**
- `UPKEEP_KINDS` obsahuje `loan_balance_check` a `loan_fixation_expired` ([milestones.ts:6](../../src/utils/milestones.ts:6)).
- Mobilní Údržba zná jen vlastních pět druhů ([upkeep.ts:9](../../src/mobile/lib/upkeep.ts:9)) a „Co vás čeká“ bere jen `agenda`.
- Připomínky › „Zůstatky a sazby úvěrů“ tak nemá žádný účinek. Cílová cesta už existuje v `milestone-path.ts:31`.

**B4 (S): Řádky Údržby „Je čas na výpis z …“ a „Nezařazené“ nejdou skrýt ani odložit** (nemají `milestone`). Účet jednou naimportovaný a dál vedený ručně tak vyrobí trvalou kartu. Desktop takový řádek vůbec nemá.

**B5 (S): Počet účtů v Cashflow si odporuje.** *(Viděno naživo.)*
- Podtitul ukazuje „Skutečné příjmy a výdaje z 1 účtu“, zatímco menu nabízí „Účty (3)“.
- `actuals.accountCount` počítá jen účty s pohybem v daném období.
- Doporučení: ukazovat rozsah filtru, ne počet účtů s pohyby.

**B6 (S): „Nezařazené“ počítá na různých místech jiné množiny.**
- Fronta, Údržba i řádek ve Finance vynechávají odložené a nezaúčtované platby. Filtr v ledgeru je zahrnuje.
- „Nechat“ (odložení na 30 dní) existuje jen na telefonu a nedá se vrátit ani zobrazit jako seznam.

**B7 (N): Odznak změny bez prahu po zaokrouhlení.** *(Viděno naživo: „▼ 0,0 %“ červeně pro −166 Kč.)* Mobil rozhoduje podle `ratio > 0` ([ChangeLine.tsx:30](../../src/mobile/components/wealth/ChangeLine.tsx:30), `Hero.tsx:136`), desktop podle `changeDirection` (`utils/change.ts:8`). Glyf ▲/▼ navíc VoiceOver přečte nahlas.

**B8 (N): Stejný odznak znamená na sousedních obrazovkách něco jiného.** Na Majetku má řádek „Akcie a ETF“ hodnotu „▼ 5,6 %“, což je změna za 30 dní. Řádky pozic o obrazovku dál ukazují „▲ 61,8 %“, což je zisk od nákupu. Ani jeden řádek neříká, k čemu se změna vztahuje.

**B9 (N): Výchozí období Cashflow.** Desktop otevře měsíc nejnovější transakce. Telefon nejdřív načte prázdný aktuální měsíc a pak přeskočí (`?o=-1`, naživo bliknutí). Když jsou data starší než 12 měsíců, zůstane telefon na prázdném měsíci.

**B10 (N): „oceněno {{datum}}“ u ostatních aktiv je ve skutečnosti `updatedAt`.** Každý nákup nebo prodej ho posune na dnešek, i když cena zůstává stará ([OtherAssets.tsx:144](../../src/mobile/pages/OtherAssets.tsx:144)).

**B11 (N): Zaokrouhlení v Cashflow.** Všechny částky reportu jsou v celých jednotkách, takže 12,40 € se ukáže jako 12 € a řádky nedají dohromady součet. Desktop používá ISO minor unit (`formatReportMoney`).

**B12 (N): Sdílená chyba os grafů.** *(Viděno naživo.)* Popisky „11,2 mil.“ a „15,9 mil.“ se na ose opakují dvakrát. Kompaktní formát s max. 1 desetinným místem nestačí na krok `niceTicks`. Týká se i desktopu: `MoonyLineChart`, `MoonyBarChart`, `MoonyBandChart` a [ScrubChart.tsx:167](../../src/mobile/components/ScrubChart.tsx:167).

## C. UX problémy samotného telefonu

**C1 (V): Formuláře převzaté z desktopu.**
- 20 dialogů (nákup a prodej akcií a krypta, účet, úvěr, pojistka, nemovitost, ruční cena…) se na telefonu vykreslí jako sheet, ale uvnitř zůstanou desktopové:
  - `type="number"` bez `inputMode`;
  - písmo 13 px bez Dynamic Type ([index.css:340](../../src/index.css:340));
  - žádné chytré výchozí hodnoty podle §10.5.
- Telefonních formulářů (`AmountField`, `DateField`) je jen osm.
- Doporučení: převést nejčastější formuláře (nákup/prodej, přidání účtu, ruční transakce z A1). Do té doby dát `Input type="number"` v telefonním shellu `type="text" inputMode="decimal"` a `parseAmountInput`.

**C2 (S): Gesta.**
- **Hlavní řádky nemají swipe ani dlouhý stisk.** Platí to pro účty, úvěry, pojistky, pozice, kryptoměny, sledované tituly, nemovitosti a ostatní aktiva. Všechny akce jsou až v detailu.
- **Dlouhý stisk se nedá objevit.** Kde existuje (události úvěru, dokumenty, dluhopisy, náklady), je `···` jen pro čtečku (`sr-only`, [RowMenu.tsx:42](../../src/mobile/components/RowMenu.tsx:42)), takže vidící uživatel o něm neví.
- **Swipe doleva znamená všude něco jiného:**

  | Obrazovka | Swipe doleva |
  |---|---|
  | Ledger, rozpad Cashflow | Kategorie |
  | Fronta k zařazení | Nechat (bez vrácení) |
  | Co vás čeká | Skrýt |
  | Rozpočty | Smazat |

- Akce se spustí po 60 px bez potvrzení a destruktivní panely nejsou barevně odlišené.
- Plně principu 2 vyhovují jen rozpočty.
- Doporučení:
  - swipe doleva = kategorie všude, kde je platba;
  - „Nechat“ přesunout na jiné gesto nebo mu dát možnost „Vrátit“;
  - destruktivním panelům dát `loss`;
  - hlavním řádkům dát aspoň dlouhý stisk.

**C3 (S): Ztráta místa.**
- Po odemčení se vždy jde na Přehled ([Lock.tsx:147](../../src/mobile/pages/Lock.tsx:147)), takže otevřený detail, rozepsaný sheet i pozice ve frontě zmizí.
- Klepnutí na záložku vždy vede na její kořen ([TabBar.tsx:37](../../src/mobile/shell/TabBar.tsx:37)). Postup „účet → mrknout na Cashflow → zpět“ tak přijde o účet, filtr i posun.
- Opakované klepnutí na aktivní záložku vloží kořen jako nový záznam historie místo návratu a neodroluje nahoru.
- Doporučení: pamatovat si cestu před zamčením a poslední cestu každé záložky.

**C4 (S): Oznámení a zkratky.** Oznámení nenese cíl ([use-notification-schedule.ts:43](../../src/mobile/hooks/use-notification-schedule.ts:43)), v `src` není žádný posluchač klepnutí a nejsou registrované ani rychlé akce na ploše. Doporučení: dát cestu do `extra` a otevřít ji po odemčení, stejně jako už to dělá `ReceivedFileHost`. Jinak upravit spec §6 a §9.

**C5 (S): Sheety zahodí rozepsaný vstup.**
- Klepnutí mimo sheet, stažení dolů nebo zpět zavře každý sheet bez otázky. Nejvíc to bolí u `RecordBalancesSheet` a dlouhých formulářů.
- Na Androidu zpět desktopový dialog nezavře, ale vrátí obrazovku pod ním. `useSheetHistory` totiž používá jen telefonní `Sheet`. To je v rozporu se spec §5 a blokuje to vydání na Androidu.

**C6 (S): Chybové stavy.**
- Finance, Přehled a Majetek nemají větev pro chybu. Finance při chybě ukáže „Zatím nemáte žádné bankovní účty“ s tlačítkem pro přidání účtu.
- Detaily mají `notFound: !isLoading && !entity`, takže chyba se ukáže jako „… nenalezen“ bez možnosti zkusit znovu.
- Opakování nabízejí jen třídy, Cashflow, Rozpočty, fronta a ledger.
- Doporučení: odlišit chybu od nenalezení a mít jednu komponentu „chyba + Zkusit znovu“.

**C7 (S): Zálohy na Androidu.** Řádek Údržby „Zatím žádná záloha“, oznámení „Zálohujte Moony“ i krok prvního spuštění vedou na neaktivní tlačítko s textem „Zálohování zatím funguje jen na iPhonu“. Doporučení: na Androidu nevytvářet řádek ani oznámení, dokud `canExport` je false.

**C8 (S): Nový klíč pro obnovení jde jen zkopírovat.** Klíč vygenerovaný přes „Zapomenuté heslo?“ nejde uložit do Souborů. První spuštění i „Změnit heslo“ tu možnost mají ([Lock.tsx:547](../../src/mobile/pages/Lock.tsx:547)).

**C9 (S): Stránky Připomínky a Oznámení se překrývají.** Úvod Připomínek je převzatý z desktopu: „…na přehledu a horní lišta“. Telefon žádnou horní lištu nemá a o oznámeních, která tento seznam řídí, úvod mlčí.

**C10 (S): Limity jdou nastavit jen dlouhým stiskem.**
- Na řádcích Cashflow se limit nastavuje jen dlouhým stiskem, protože `···` je `sr-only`.
- Rozpočet jde smazat jen swipem nebo dlouhým stiskem.
- Poznámka „Rozpočty: 5 v mezích · 0 nad limitem · 0 blíží se limitu“ vede na stránku, která čerpání neukazuje. Navíc vypisuje i nuly.
- Doporučení: přidat viditelné „Nastavit limit“ a filtr „Nad limitem“ místo poznámky.

**C11 (N): Drobnosti.**
- Řádky „Poslední pohyby“ na Přehledu nejdou otevřít.
- „Nejbližší platby“ ve Finance obsahují jen pojistné, splátky úvěrů (24 800 Kč měsíčně) v nich chybí. Nadpis slibuje všechny platby.
- Kroužky na grafu Přehledu nemají legendu.
- Statistiky na šířce 390 se oříznou: „+ 75,0 % · realiz…“, „TWR · MWR + 4…“.
- Účet zobrazí sekci Toky až po načtení, takže seznam poskočí.
- Údržba začíná sbalená, takže slovesná tlačítka jsou o klepnutí dál.
- Chybí pull-to-refresh (spec §8.8) i aktualizace cen v detailu pozice.
- Datum transakce se staví na čtyřech místech podle různých pravidel.
- Patička `TransactionSheet` má vlevo „Smazat transakci“, kde jinde je „Zrušit“.
- Toasty jsou posunuté nad tab bar i na obrazovkách bez něj.
- Na zámku chybí „Obnovit ze zálohy“.

## D. Terminologie (cs)

Princip 6 specifikace chce stejná slova jako na desktopu. Nejvýraznější páry:

| Pojem | Telefon | Desktop |
|---|---|---|
| Položka ledgeru | „Pohyby“, „3 pohyby“ (`mobile:account.ledger.title`, `cashflow.moves`) | „Transakce“, „N transakcí“; v rozpadu Cashflow na telefonu taky „transakcí“ |
| Nezařazené | „Nezařazené platby“, „Bez kategorie“ | „Nezařazené transakce“, „Nezařazené“; sdílené `reports` „Nezařazeno“ vs. `budgeting` „Nezařazené“ |
| Volná hotovost | „Volné peníze · na účtech“ | „Volné prostředky · na bankovních účtech“ |
| Aktiva | „Aktiva“ | „Celková aktiva“ |
| Období změny | „za 30 dní“, „za 3 měsíce“ (je to 90 dní), „letos“, „za rok“ | „za posledních 30 dní“, „za posledních 90 dní“, „od začátku roku“, „za poslední rok“ |
| Správa limitů | „Spravovat rozpočty“ | tlačítko „Spravovat limity“, dialog „Spravovat rozpočty“ (nejednotný i desktop) |
| Stav limitu | „blíží se limitu“, „nad limitem“ | „Blíží se limitu“, „Nad limitem“, „Limit vyčerpán“ |
| Dividendy | „Dividendy / rok“, „Dividendy“ (je to odhad) | „Odhad dividend“ |
| Ruční cena | „Upravit cenu“ (akcie) | „Zadat kurz ručně“ (krypto) |
| Technická pole transakce | „ID banky“, „Valuta“, „Soubor“, „Řádek“ | „ID bankovní transakce“, „Datum valuty“, „Původní soubor“, „Řádek souboru“ |
| „Poznámka“ v detailu transakce | je to popis od banky, ne poznámka uživatele | — |

Další nejednotnosti uvnitř telefonu:
- „Vymazat hledání“ vs. „Smazat hledání“.
- „Hotovo“ vs. „Zavřít“ na zavírání sheetu.
- „Zrušit filtry“ vs. „Zrušit filtr“.
- „Odstranit událost“ vs. „Smazat …“ všude jinde.
- „Upravit účet“ / „Upravit“ / „Upravit údaje“.
- Titulky potvrzení „Smazat úvěr“ a „Smazat pojistku“ jsou bez otazníku, ostatní ho mají.
- Krypto přepíná „Nerealizovaný výnos/ztráta“, akcie ne.
- Potvrzení smazání obchodu zní „…transakci typu koupit…“, chyba sdílená s desktopem.

Paritou locales projdou všechny mobilní klíče (`locales.test.ts`, 193 testů zelených). Kvalitu překladů jsem nehodnotil.

## E. Nedodělky v kódu

- Finance `···` › „Export“ jen ukáže toast „Export přijde později.“ ([Finance.tsx:257](../../src/mobile/pages/Finance.tsx:257)). Export z Cashflow a tříd Majetku přitom funguje.
- Rozpočty ukazují štítek „ukončen“, ačkoli vlastník rozhodl, že stav ukončení není. Dřívější intervaly limitu se nezobrazují (`Budgets.tsx:261`).
- Zastaralé komentáře:
  - `use-cashflow.ts:81` (odkazuje na smazaný `Budgeting.tsx` a sdílené klíče cache);
  - `Bonds.tsx` (klepnutí neotevře editaci, ale rozbalí řádek);
  - `Hero.tsx` (skrývání částek);
  - `PropertyDetail.tsx:205`.
- `STALE_BALANCE_DAYS = 30` je zkopírované z Rustu ([upkeep.ts:5](../../src/mobile/lib/upkeep.ts:5)) a časem se rozjede.

## F. Spec vs. realita (rozhodnutí pro vlastníka)

Tyto věci spec slibuje pro v1 a kód je nemá. Je třeba buď dodělat, nebo spec upravit:

- seznam pravidel, Kategorie, Viditelnost tříd a „Co se počítá do přehledu“ (§7, §8.9);
- přidání transakce a vrácení importu (§8.4, §8.6);
- oznámení a zkratky, které otevírají cíl (§6, §9);
- pull-to-refresh a vyhledávání stažením dolů (§8.8);
- karta Začínáme (§8.3);
- export JSON, země v profilu (§8.9);
- skrývání částek: oko na Přehledu a „skrýt při spuštění“ ve spec zůstaly (§6, §8.3, §8.9, D6), přestože je vlastník zrušil.

Opačný směr, kde se desktop změnil a spec telefonu se nedotkla:

- **Cashflow s rozpočty (2026-10-28).** Desktop odstranil export i souhrnnou poznámku k rozpočtům, telefon obojí má. Je potřeba rozhodnout, jestli telefon zůstane vlastní cestou.
- **Dividendy, GIPS a splity.** Viz A3, A10 a A11. Bez zápisu do §7 je to nerozhodnutá vynechávka.

## G. Co sedí

- **Čísla mají jeden zdroj.** Čisté jmění, výnosy pozic (včetně dividend v realizovaném), TWR/MWR, dluhopisy, nemovitosti i ostatní aktiva počítá stejný backend a stejné utility jako desktop. Telefon nedělá vlastní finanční matematiku a nepřevádí 1:1. Nativní měny jsou zobrazené se „≈“ převodem a poznámkou o chybějícím kurzu.
- **Mazání má vždy potvrzení.** Všechna mazání jdou přes ConfirmSheet a mazání s odchodem z obrazovky drží pohled.
- **Kategorizace jedné platby je věrný port desktopu:** zapamatování, „Zařadit i dalších N plateb“, přesměrování pravidla, „Vrátit“.
- **Limit sheet odpovídá desktopové logice:** Od/Do, „Bez konce“, nejnovější vyhrává, varování před přepsáním. Totéž platí pro mazání rozpočtů.
- **Zámek, první spuštění, změna hesla a biometrie** odpovídají desktopovým tokům (změna hesla je dokonce bohatší).
- **Formuláře převzaté z desktopu** mají stejná pole, validaci a výchozí hodnoty. Fee pole u obchodů funguje.
- **Kód je čistý:** žádné natvrdo zapsané texty, popisky u ikonových tlačítek, 44pt cíle, tokeny pro tmavý režim a test na barevné literály.

## Ověřit na zařízení (iPhone, později Android)

1. `type="number"` v převzatých formulářích s českou klávesnicí: jde napsat „12,5“? (C1)
2. Co ukáže Přehled, Finance a Majetek, když dotaz selže. (C6)
3. Jestli svislý posun ve frontě omylem nespouští „Nechat“ (60px práh, nejde vrátit).
4. Převzaté dialogy při 130 % Dynamic Type a s otevřenou klávesnicí.
5. Android: zpět s otevřeným desktopovým dialogem a zpět po cyklu zamčení a odemčení.
6. Pole „počet dní pro výpověď“ v Připomínkách se ukládá jen při `blur`. Uloží se, když člověk odejde gestem zpět?

## Navržené pořadí práce

1. **Kategorizace v Nastavení (B1, A5).** Bez ní uživatel jen s telefonem nemůže opravit chybu, kterou mu telefon sám naučil.
2. **Ruční transakce a vrácení importu (A1, A2), dividendy (A3).** Uzavře se tím slib specifikace i jediný rozpor v číslech, viditelný hned v detailu pozice.
3. **Cashflow na desktopové `expenseRows` a `limitState` a jeden dotaz (B2, B5, A16).**
4. **Navigace a odolnost:** návrat po odemčení a paměť záložek (C3), chybové stavy (C6), ochrana rozepsaného sheetu a Android zpět (C5), cíle oznámení (C4).
5. **Formuláře záznamu událostí na telefonní pole (C1).** Nejdřív ověřit dopad na zařízení.
6. **Sjednocení gest (C2) a terminologie (D).** Jeden průchod texty a jedno pravidlo pro swipe.
