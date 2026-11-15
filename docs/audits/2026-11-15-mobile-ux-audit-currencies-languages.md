# Mobilní Moony: UX a vizuální audit v různých měnách a jazycích (2026-11-15)

Stav: `main` @ b031aa57. Audit jen čte, v kódu se nic neměnilo. Navazuje na
[audit z 6. 11.](2026-11-06-mobile-ux-audit.md) (ten řešil nesoulad s desktopem); tento se ptá jinak:
je aplikace jednoduchá, srozumitelná a graficky vyladěná v každém jazyce a v každé hlavní měně?

Měřítko: obrazovka má jedno hlavní sdělení, čísla jsou zarovnaná a srovnatelná, nic důležitého
nekončí třemi tečkami, stejná věc vypadá všude stejně.

## Rozsah a metoda

- **Živý průchod.** Telefonní shell (`VITE_MOONY_SHELL=mobile`, debug build) nad **kopií skutečného
  profilu** (43 pozic, 8 účtů, 3 úvěry, 3 pojistky, rozpočty, nemovitosti). Kopie běžela pod vlastním
  identifikátorem se synchronizací vypnutou (`MOONY_DEV_SYNC=0`) a po auditu je smazaná; ostrý profil
  zůstal nedotčený. Příklady v tomto dokumentu jsou zobecněné, žádná skutečná jména ani částky.
- **Okno.** 393 × 852 a 360 × 780, panel prohlížeče (Chromium), tedy ne WKWebView.
- **Kombinace.**
  - čeština + CZK: všechny čtyři záložky, detail účtu (ruční i výpisový), transakce, výběr kategorie,
    úvěr, pojistka, kategorie cashflow, limit rozpočtu, výdajové plány, Akcie, detail pozice,
    Nastavení, výběr jazyka a měny;
  - angličtina + USD, němčina + EUR, japonština + JPY: čtyři záložky a Nastavení;
  - tmavý režim: Přehled, Cashflow, Majetek (v japonštině).
- **Kód.** Souběžná rešerše `src/mobile` a sdílených formátovačů. Nálezy jen z kódu jsou označené
  „(kód)“; čísla řádků z rešerše jsem ověřil namátkou (`DayHead.tsx`, `amount-input.ts`,
  `DeleteProfileSheet.tsx`), ostatní si implementátor potvrdí při opravě.
- **Co jsem neprošel:** viz sekce G.

Závažnost: **V** = vysoká (špatná nebo nečitelná informace), **S** = střední (viditelná
nedotaženost, zmatení), **N** = nízká (kosmetika).

---

## Shrnutí: dvanáct věcí, na kterých nejvíc záleží

1. **V: S jinou hlavní měnou míchá Cashflow dvě měny v jednom řádku.** Útrata kategorie je v hlavní
   měně („$516“), limit a „zbývá“ v měně rozpočtu („CZK 12,500.00“), mezi nimi jedna lišta s 90 %.
   Čísla nejdou porovnat. → A1
2. **V: Poslední pohyby přepočítávají jednotlivé platby do hlavní měny** bez značky ≈, bez původní
   částky a zaokrouhlené na celé jednotky: platba 50 Kč je „− 2 €“. → A2
3. **V: Důležité údaje končí třemi tečkami.** Částka uprostřed čísla v agendě, datum v Údržbě,
   podmínky fixace v dlaždici úvěru, podřádky tříd majetku. Platí ve všech jazycích. → C1
4. **S: Řádky jsou v nativní měně, součty sekcí v hlavní**, a „≈“ mají jen účty. Úvěry, pojistky,
   platby a nemovitosti převod nemají, takže součet v hlavičce nejde zkontrolovat. → A3
5. **S: Desetinná místa nemají pravidlo.** Cashflow ukazuje haléře všude („12 500,00 Kč“), zbytek
   aplikace nikdy, ani u eur a dolarů. → A4
6. **S: Řádky kategorií v Cashflow nejsou zarovnané.** Řádek bez limitu má `···` a užší lištu,
   takže částky mají dvě pravé hrany; stavový řádek se láme mezi „Zbývá“ a částkou. → D18
7. **S: Karta Údržba je nejslabší komponenta Přehledu.** Titulky se lámou na 2–3 řádky, podřádek je
   uříznutý, tlačítka mají různou šířku a styl. → D3
8. **S: Během načítání ukazuje Majetek nepravdu:** „0 pozic“, „0 emisí“, „0 položek“ a chybí celá
   sekce Nemovitosti, přestože rozložení nad tím už ukazuje 31 %. → E1
9. **S: Jedna transakce má na třech obrazovkách tři podoby** (jiný titulek, jiná ikona, jiný
   podřádek) a seznamy míchají čtyři styly avatarů. → C3, C4
10. **S: Angličtina znamená americká data** („11/8“, „10/16“) bez možnosti volby; pro evropského
    uživatele v angličtině je to dvojznačné. → B1
11. **S: Němčina dělí popisky dlaždic uprostřed slova** („Gesamtvermö-gen“, „Verbindlichkei-ten“) a
    jedno slovo „Vermögen“ označuje tři různé pojmy. → B2, B3
12. **S: Patička sheetu nemá pravidlo.** Detail transakce má jako hlavní černé tlačítko „Zavřít“,
    výběr kategorie textové „Zrušit“, limit „Zrušit + Uložit“. → C7

Co je dobře a nemá se rozbít: v žádné z prošlých kombinací nic nepřetéká do stran (ani při 360),
dolní lišta se vejde ve všech čtyřech jazycích, kompaktní čísla na osách jsou správně lokalizovaná
(„8,4 mil.“, „29K“, „6000万“, „1.54億“), tmavý režim je čistý, Nastavení a oba pickery jsou
přehledné, hero s číslem a změnou funguje.

---

## A. Měny

Stav dnes: hlavní měna se píše symbolem („Kč“, „$“, „€“, „￥“), každá jiná kódem („CZK 620,000“
v angličtině, „620 000 CZK“ v češtině). Nativní částky zůstávají nativní, souhrny se převádějí.
Pravidlo je rozumné, ale neuplatňuje se všude stejně.

| # | Záv. | Nález | Kde | Doporučení |
|---|---|---|---|---|
| A1 | V | **Rozpočtový řádek míchá měny.** Útrata v hlavní měně, limit a „Zbývá …“ / „… nad limitem“ v měně rozpočtu, lišta ukazuje procenta. Viděno v USD, EUR i JPY. | Cashflow → Výdaje podle kategorií; `src/mobile/components/cashflow/ExpenseCategoryRow.tsx` | V jednom řádku jedna měna. Buď vše v měně limitu (útrata, limit, zbývá) a převod jen v hlavičce sekce, nebo vše převést a limit označit „≈“. |
| A2 | V | **Jednotlivé platby převedené do hlavní měny.** „Poslední pohyby“ ukazují platbu z korunového účtu jako „− $8“ / „− 7 €“ / „− ￥1,262“, bez ≈ a bez původní částky. Na detailu účtu je táž platba v korunách. | Přehled; [Overview.tsx:355](../../src/mobile/pages/Overview.tsx:355), `src/mobile/lib/signed-amount.ts` | Transakce vždy v měně účtu (nativní hodnota je autoritativní, ADR 0012). Převod nejvýše jako druhý, šedý údaj s „≈“. |
| A3 | S | **Nativní řádky pod převedeným součtem.** Finance: účty mají „≈ hlavní měna“, úvěry, pojistky a Nejbližší platby ne; hlavičky („nesplaceno …“, „… ročně“) jsou v hlavní měně. Majetek: investice jsou převedené, nemovitosti nativní bez ≈, součet sekce v hlavní měně. | Finance, Majetek; `Finance.tsx:545` (jediné místo s `finance.trail.approx`) | Jedno pravidlo pro každý řádek entity: nativní částka nahoře, „≈ převod“ pod ní, kdykoli se měna liší od hlavní. |
| A4 | S | **Desetinná místa bez pravidla.** Cashflow: vždy dvě („+ 173 768,98 Kč“, limit „12 500,00 Kč“). Jinde nikdy žádná, ani u EUR a USD („− 2 €“ za 2,06 €). Detail kategorie v Cashflow ukazuje platby bez desetin přímo pod součtem s desetinami. | `shared/currencies.ts:73`, `signed-amount.ts:14`, Cashflow | Souhrny (hero, dlaždice, hlavičky, limity, osy) bez desetin. Jednotlivá transakce přesně: podle minor units měny, „,00“ se u celé částky vynechá. |
| A5 | S | **Symbol v řádku, kód v poli.** Tentýž účet: „1 234 Kč“ v řádku, „CZK“ ve vstupním poli (limit, zůstatek). V poli navíc „12500“ bez oddělovače tisíců. | `src/mobile/components/AmountField.tsx:111`, `src/lib/format.ts:106` | Pole používá stejné označení měny jako okolí a formátuje tisíce při opuštění pole. |
| A6 | S | **Detail pozice střídá měny dlaždici od dlaždice.** Hodnota v měně titulu s poznámkou „v USD“, Cena za kus v hlavní měně (bez ≈), Vloženo v hlavní, obchody v měně titulu. Řádek nákupu ukazuje stejnou částku dvakrát („− 9 163 USD“ a pod tím „9 163,00 USD“). | `wealth/PositionStats.tsx:90`, `wealth/TradeRow.tsx:88` | Cena v měně burzy (tak ji uživatel zná), převod jako poznámka s ≈. Druhý řádek obchodu má nést převod do hlavní měny, ne opakování. |
| A7 | S | **Jediný oddělovač je vždy desetinný.** Němec napíše „1.234“ a uloží se 1,234; Angličan „1,234“ stejně. Rozliší se jen zápis s oběma oddělovači. (kód, ověřeno) | [amount-input.ts:12](../../src/mobile/lib/amount-input.ts:12) | Osamocený oddělovač následovaný přesně třemi číslicemi číst jako tisíce, pokud je to skupinový oddělovač jazyka. |
| A8 | N | **Graf „Celkem na účtech“ v cizí hlavní měně kolísá**, i když se zůstatky nezměnily (kurz). Změna „▲ 4,3 %“ pak vypadá jako přírůstek peněz. | Finance, Majetek hero | Když jsou účty v jiné měně než hlavní, přidat k řádku změny „včetně kurzu“. |
| A9 | N | „0 EUR“ s podřádkem „≈ 0 Kč“. | Finance → Účty | U nuly převod vynechat. |
| A10 | N | **Výběr hlavní měny:** abecedně podle kódu, aktuální měna uprostřed seznamu; po uložení jen obecný toast „profil uložen“. Ukázka v angličtině „e.g. Kč 1,235“. | `sheets/CurrencyPickerSheet.tsx`, `Settings.tsx:386` | Nahoře „Časté“ (současná, EUR, USD, měna země profilu). Toast „Hlavní měna je teď X, souhrny se přepočítaly“. |
| A11 | N | Práh kompaktního zápisu je pevné číslo (1 000 000), definované dvakrát; pro JPY a HUF je to málo, pro EUR hodně. (kód) | `lib/stat-money.ts:5`, `Overview.tsx:57`, `Stocks.tsx:396` | Jedna konstanta, práh podle počtu číslic, ne hodnoty. |
| A12 | N | Osy `CashflowBars` a `PlanMonthBars` mají pevnou šířku 54 px a vlastní formát, ne `useTickFormatter`. Naživo jsem ořez neviděl. (kód) | `cashflow/CashflowBars.tsx:70`, `plans/PlanMonthBars.tsx:70` | Použít stejný formátovač a měřenou šířku jako `ScrubChart`. |

---

## B. Jazyky a formáty

| # | Záv. | Nález | Kde | Doporučení |
|---|---|---|---|---|
| B1 | S | **Angličtina = americká data.** „11/15/2026“, „10/16“, „Last import 10/10“, „next payment 1/9/2027“. Jazyk a region nejdou zvolit zvlášť. | všude (Intl s `en`) | Krátká data psát s měsícem slovem („Nov 8“, „Jan 9, 2027“), nebo řídit formát regionem systému či zemí profilu. |
| B2 | S | **Němčina: popisek dlaždice dělený uprostřed slova** („Gesamtvermö-gen“, „Verbindlichkei-ten“), třetí dlaždice jednořádková. | Přehled; [StatRow.tsx:48](../../src/mobile/components/StatRow.tsx:48) (`hyphens-auto`) | Popisek dlaždice má v každém jazyce vyjít na jeden řádek: kratší texty („Vermögen“, „Schulden“) a test délky v `locales.test.ts`. |
| B3 | S | **„Vermögen“ znamená tři věci:** název záložky (Majetek), střed prstence (Aktiva) a dlaždice „Gesamtvermögen“ (Celková aktiva). Text ve středu prstence se dotýká jeho okraje. | de `mobile.json` | Rozlišit pojmy (Vermögen / Aktiva / Gesamtaktiva); střed prstence omezit šířkou a zmenšovat písmo. |
| B4 | S | **Japonský pozdrav se nevejde do titulku** (velký titulek 298 px na 277 px, sbalený končí „Filip…“). | Přehled, ja | Viz C9: sbalený titulek = název záložky. Velký titulek smí zalomit na dva řádky (už se láme) a nesmí se krátit. |
| B5 | S | **Potvrzovací slovo „DELETE“ je anglicky ve všech jazycích.** (kód, ověřeno) | [DeleteProfileSheet.tsx:16](../../src/mobile/sheets/DeleteProfileSheet.tsx:16) | Přeložené slovo na jazyk, porovnání bez ohledu na velikost písmen. |
| B6 | N | Čas v angličtině s úvodní nulou: „Locked today at 08:51 AM“, „04:32 PM“. | Zámek, Nastavení | `hour: 'numeric'` pro dvanáctihodinový formát. |
| B7 | N | ja `finance.perMonth` je „月 {{amount}}“ (čte se jako „měsíc 1 234“), zh má „{{amount}} / 月“. (kód) | `src/i18n/locales/ja/mobile.json:117` | „{{amount}}/月“ nebo „月額 {{amount}}“. |
| B8 | N | Nativní `<input type="date">` a `type="month"` se řídí jazykem systému, ne aplikace. V limitu rozpočtu je prázdný měsíc vidět jako „--------- ----“ s uříznutou ikonou. (kód + naživo v Chromiu) | [DateField.tsx:33](../../src/mobile/components/DateField.tsx:33), `cashflow/MonthField.tsx` | Vlastní zobrazení hodnoty přes `fmt`, prázdný stav slovem („Bez omezení“). Ověřit na zařízení. |
| B9 | N | Věty skládané spojováním (`${t(a)} ${t(b)}`, „Akce: “ + název) a seznamy přes `', '`; v ja a zh patří „、“ a „：“. (kód) | `LedgerRow.tsx:97`, `ImportHistorySheet.tsx:85`, `settings/SyncSection.tsx:227` aj. | Jeden klíč s interpolací; seznamy přes `Intl.ListFormat`. |
| B10 | N | CSV export píše měsíc lokalizovaně („10/2026“, „10.2026“). (kód) | `Cashflow.tsx:215` | `yyyy-MM`. |
| B11 | N | Plnošířkový „￥“ v japonštině působí těžce vedle latinkových číslic a za znaménkem je mezera („− ￥247,949“). | ja | Zvážit „¥“ (narrow) i pro ja; znaménko formátovat přes Intl (`signDisplay`), ne předřazeným řetězcem. |

---

## C. Průřezové vizuální nálezy

| # | Záv. | Nález | Příklady | Doporučení |
|---|---|---|---|---|
| C1 | V | **Tři tečky na důležitých údajích.** | Agenda: „Splatnost · vrátí se 1 2…“ (částka uprostřed čísla), „Termín pro doručení výpově…“. Údržba při 360: „Poslední import 1…“. Akcie hero: „… za posledních 30 dní · vložen…“. Dlaždice: „+ 37,5 % realizováno…“, „při 3 % splátka …, při 1 %…“. Majetek: „43 pozic · MSFT,…“, „nejbližší splatn…“, „nájem … / mě…“. Účty: „banka · CZK · výpi…“. | Pravidlo do design systému: podřádek nese nejvýše dvě fakta; částka a datum se nikdy nekrátí (krátí se text před nimi); co se nevejde, patří do detailu. Kontrolu z `mobile.html` (seznam textů s výpustkou) pustit nad skutečnými obrazovkami. |
| C2 | S | **Dlaždice nedrží společné účaří.** Hodnoty jsou zarovnané odspodu, takže dvouřádková poznámka nebo popisek posune číslo o řádek. | Přehled (třetí dlaždice výš než první dvě), úvěr („Zbývá splátek“), Akcie. | Pevná mřížka dlaždice: popisek 1 řádek, hodnota, poznámka 1 řádek. Delší poznámka nepatří do dlaždice. |
| C3 | S | **Čtyři styly avataru v jednom seznamu.** Šedé iniciály, ikona banky nebo bankovky, zelená ikona převodu, barevné logo (jen u části akcií). Barevné ikony kategorií existují jen v Cashflow a ve výběru kategorie. Dva účty různých jmen mají stejné iniciály. | Detail účtu, Akcie, Finance | Transakce: ikona kategorie (stejná jako v Cashflow). Akcie: logo, jinak monogram ve stejném tvaru a barevnosti. Účty: logo banky nebo ikona typu účtu. |
| C4 | S | **Titulek transakce se liší podle obrazovky.** Přehled: popis platby. Detail účtu: číslo protiúčtu, popis až v podřádku. Detail kategorie: číslo protiúčtu a v podřádku surová poznámka z výpisu („29102026 \| Platba kartou“). | `LedgerRow`, `cashflow/DrilldownRow.tsx` | Jedna funkce pro titulek: pojmenovaná protistrana → popis → číslo účtu. Surová poznámka banky jen v detailu transakce. |
| C5 | S | **Pět podob data na dosah ruky.** „15. 11. 2026“, „30. 11.“, „8/2029“, „27. října 2026“, na ose „listopad 25“ a „březen 38“ (čte se jako den). | úvěr, pojistka, transakce | Tabulka formátů v design systému (den, den s rokem, měsíc s rokem, osa). Osa: „11/25“ nebo jen rok; fixace „srpen 2029“. |
| C6 | S | **Jednotky období nejsou jednotné:** „/ měs“, „/ měsíc“, „/ rok“, „ročně“, „měsíčně“. Pojistky mají v jednom sloupci částky za měsíc i za rok bez jednotky. | Finance, Majetek | Jeden tvar („2 270 Kč / měs.“, „11 216 Kč / rok“) vždy u částky. |
| C7 | S | **Patička sheetu.** „Zavřít“ jako hlavní černé tlačítko (transakce), textové „Zrušit“ (výběr kategorie, jazyk, měna), „Zrušit + Uložit“ (limit). | `sheets/*` | Čtecí sheet nemá hlavní tlačítko (zavírá se tažením a „Hotovo“ v hlavičce). Hlavní tlačítko jen pro akci, vždy vpravo, „Zrušit“ vlevo. |
| C8 | N | **Afordance řádků.** Šipku vpravo mají jen řádky v Nastavení; jinde nic. „Majetek →“ je textová šipka; „Poslední pohyby“ a „Co vás čeká“ odkaz na vše nemají. | Přehled, Finance, Majetek | Rozhodnout jednou: buď chevron všude, kde řádek vede dál, nebo nikde. Odkaz v hlavičce sekce jednotně. |
| C9 | N | **Sbalený titulek Přehledu je pozdrav** („Dobré ráno, …“), ostatní záložky ukazují svůj název. | Přehled | Sbalený titulek „Přehled“. |
| C10 | N | **Dlouhý název se v navigaci detailu krátí** („Pojištění odpovědnos…“), protože ho svírá „‹ Finance“ a `···`. | detail pojistky, pozice | Při dlouhém názvu zpět jen šipkou, nebo velký titulek pod lištou jako na záložkách. |
| C11 | N | **Lepivá hlavička dne má plochou barvu na přechodovém pozadí:** viditelný pruh a ostrá hrana karty pod ním. | [DayHead.tsx:11](../../src/mobile/components/DayHead.tsx:11) (`bg-canvas`) | Průsvitné pozadí s rozostřením, nebo pozadí stránky bez přechodu pod seznamem. |
| C12 | N | **Dva úchyty nad sebou**, když se nad sheetem otevře druhý. | výběr kategorie z transakce | Spodní sheet při překrytí úchyt skryje. |
| C13 | N | **Legendy grafů.** Úvěr má pět položek ve dvou řádcích a dvě skoro stejné kroužkové značky. Toky na účtu nemají legendu vůbec. Rozložení majetku se láme 2 + 3. | úvěr, účet, Majetek | Nejvýše tři položky v legendě; události grafu vysvětlit při dotyku. |
| C14 | N | **Nabídka `···` nemá ikony** a „Přidat transakci“ je schovaná v ní, zatímco „Zaznamenat zůstatek“ je hlavní tlačítko. | detail účtu | Ikony v nabídce; u ručně vedeného účtu „Přidat transakci“ mezi tlačítka. |

---

## D. Obrazovky

### Zámek
- **D1 N** Placeholder hesla má rozpal určený pro tečky hesla („H e s l o“, „P a s s w o r d“).
  Rozpal jen na vyplněnou hodnotu (`:not(:placeholder-shown)`). `src/mobile/pages/Lock.tsx`
- **D2 N** Nabídka odemykání otiskem vyjede hned po prvním odemčení přes rozpracované načítání.
  Počkat na vykreslený Přehled, nebo ji nabídnout řádkem v Údržbě.

### Přehled
- **D3 S Údržba.** Titulek se vedle tlačítka láme na dva řádky (při 360 na tři: „5 zůstatků /
  starších než / měsíc“), podřádek je výčet účtů uříznutý po prvním, tlačítka mají různou šířku a
  dva styly, vedle každého ještě `···`. Řádky proto mají různou výšku a nic není zarovnané.
  [UpkeepCard.tsx:236](../../src/mobile/components/UpkeepCard.tsx:236)
  → Řádek jako jeden cíl klepnutí: ikona, titulek na jeden řádek („5 zůstatků k aktualizaci“),
  podřádek počtem („5 účtů“), vpravo jedna akce stejné šířky nebo jen chevron. Odložit a skrýt do
  swipe a dlouhého stisku, které už existují.
- **D4 N** Údržba je nad čistým jměním. Hlavní číslo aplikace by mělo být první; Údržba pod ním,
  nebo sbalená na jeden řádek („3 věci k údržbě“).
- **D5 N** Prstenec rozložení má uprostřed jen slovo „Aktiva“ a legenda jen procenta. Do středu
  celkovou částku, do legendy částky.

### Finance
- **D6 S** Podřádky účtů jsou přetížené: „banka · měna · výpis datum“ vlevo a „± částka · 30 dní“
  nebo „výpis 36 dní starý“ vpravo, levá část se krátí. Měna se opakuje i tam, kde je stejná jako
  hlavní. → Vlevo jen banka (měnu jen když se liší), vpravo jeden údaj.
- **D7 N** Poznámka v hlavičce „90 dní“ u Nejbližších plateb nedává smysl bez kontextu
  („příštích 90 dní“).

### Detail účtu
- **D8 S** Účet bez zadaného zůstatku nemá hero vůbec: stav „0 Kč“ ze seznamu tu není nikde vidět a
  obrazovku otevírají dvě tlačítka přes celou šířku. → Hero i bez historie („Zůstatek nezadán“ s
  akcí), tlačítka vedle sebe pod ním.
- **D9 S Toky.** První sloupec a jeho popisek jsou uříznuté hranou karty („b“ místo „Dub“), sloupce
  nemají legendu (které jsou příchozí), vybraný měsíc není na ose zvýrazněný a souhrn pod grafem se
  láme se sirotkem („… · 83 / pohybů“). `src/mobile/components/FlowStrip.tsx`
- **D10 N** Podřádek navigace u ručního účtu je jen „CZK“. „Průměrně za měsíc“ se počítá i z
  jediného měsíce.

### Detail transakce
- **D11 S** Datum je tu dvakrát (pod názvem slovy a v řádku „Datum“ číslicemi), protistrana také
  (titulek a řádek „Protistrana“). U příjmu se nabízí „Výdajový plán“. „Význam pohybu“ s popiskem
  „Příjem · Role kategorie“ je žargon. V Podrobnostech jsou technické údaje uříznuté („ID bankovní
  transakce 01a1…“, název souboru). → Hlavička nese protistranu, datum a částku jednou; řádky jen
  to, co jde změnit; technické údaje na jedno klepnutí zkopírovat celé.

### Výběr kategorie
- **D12 S** Současná kategorie není označená ani nahoře. Oba přepínače jsou předem zapnuté, takže
  jedno klepnutí na kategorii vytvoří pravidlo a přeřadí dalších deset plateb. Text přepínače
  („Tento účet · Příchozí · číslo účtu. Spolehlivé účty platí pro různé názvy protistrany; jinak
  pouze tento příjemce.“) je nesrozumitelný a patička zabere čtvrtinu sheetu.
  → Označit vybranou. Hromadný dopad nabídnout až po výběru jednou větou s „Vrátit“
  („Zařazeno. Zařadit i 10 dalších plateb od téhož plátce?“).

### Úvěr
- **D13 S** Dlaždice „Konec fixace“ má v poznámce celý scénář („za 2 roky 9 měsíců · při 3 %
  splátka …, při 1 %…“), který se uřízne. Patří do vlastní sekce „Po konci fixace“.
- **D14 N** „platí, dokud ho nezměníte“ je na obrazovce třikrát (upozornění a dva řádky událostí).
  Osa grafu „listopad 25“ viz C5, legenda viz C13.

### Pojistka
- **D15 S** Detail nemá hero, začíná tabulkou faktů; účet a úvěr hero mají. → Hero s pojistným a
  příští platbou.
- **D16 N** „Limity krytí celkem 200 mil.“ sčítá nesouvisející limity (majetek + zdraví). Graf
  „Zaplaceno na pojistném“ promítá smlouvu na dobu neurčitou dvanáct let dopředu. Sekce Poznámky je
  holý text bez karty.

### Cashflow
- **D17 S** Čtyři řady ovládání nad obsahem (podzáložky, Měsíc / Čtvrtletí / Rok, krokování měsíce,
  filtr s řazením) ve čtyřech různých stylech a titulek „Cashflow“ hned nad podzáložkou „Cashflow“.
  → Období a krokování do jednoho řádku; filtr limitů až u seznamu.
- **D18 S Řádek kategorie.** `cashflow/ExpenseCategoryRow.tsx`
  - Řádek bez limitu má `···` a kratší lištu, řádek s limitem ne: částky končí na dvou různých
    svislicích.
  - Stavový řádek se láme mezi „Zbývá“ a částkou; u jiných řádků se vejde.
  - „↑ Nad limitem 170 % · − 2 796 Kč nad limitem“: dvakrát totéž a minus u přečerpání.
  - Lišta znamená dvě věci: u řádku s limitem čerpání limitu, bez limitu podíl na výdajích.
  - → Jedna svislice pro částky (`···` pryč, akce přes klepnutí a swipe). Stav jednou větou
    („Zbývá 1 229 Kč z 12 500“, „Přečerpáno o 2 796 Kč“). Lišta jen u limitu; podíl na výdajích
    stačí jako procento v podřádku.
- **D19 S** Detail kategorie je jen seznam plateb: chybí součet, limit a vývoj, tedy to, kvůli čemu
  se sem jde.
- **D20 N** V grafu není vybraný měsíc popsaný na ose. „Nezařazené kredity se počítají jako příjem“:
  „kredity“ nahradit „příchozí platby“.

### Limit rozpočtu
- **D21 S** Kategorie je na sheetu třikrát (věta, karta, kontext). Pole ukazuje „12500“ a „CZK“
  (A5). Prázdný měsíc viz B8. Nápověda mluví jazykem implementace („Pozorovaný měsíční průměr … z 6
  celých měsíců. Úplnost výpisů není ověřena.“, „Bez měsíce „do“ platí limit napořád a nahradí i
  pozdější změny.“). → „Obvykle utratíte 12 900 Kč měsíčně.“, „Platí, dokud ho nezměníte.“

### Výdajové plány
- **D22 N** Prázdný rok říká „založte první plán“, i když loni plány byly. Tlačítka prázdného stavu
  jsou užší než tlačítka jinde.

### Majetek
- **D23 S** „za posledních 30 dn“: text za odznakem změny je oříznutý, když je odznak širší
  („▼19,8 %“). Tatáž slova se opakují na každém řádku; období stačí jednou v hlavičce.
- **D24 N** Řádky nemovitostí mají vpravo datum a změnu bez popisku (ocenění? k čemu?).

### Akcie a detail pozice
- **D25 S** Podřádek hero se krátí (C1). Dlaždice „Zhodnocení letos“ ukazuje při načítání „—“ bez
  vysvětlení; popisek „zisk/ztráta od nákupu“ visí mezi řazením a lupou.
- **D26 S** Detail pozice bez dopočítané historie ukáže „bez historie“ a pod tím přepínač období bez
  grafu. → Přepínač skrýt; když se historie teprve dopočítává, říct to.
- **D27 N** „Yahoo Finance“ je jako zdroj na každém řádku dividendy. Stačí jednou v patičce.

### Nastavení
- **D28 N** E-mail je dvakrát pod sebou (hlavička a řádek Profil). Řádky s podřádkem a bez něj mají
  různou výšku.

---

## E. Načítání a odezva

Časy jsou z debug buildu přes vývojový most, na zařízení budou kratší. Chybné mezistavy ale na
rychlosti nezávisí.

| # | Záv. | Nález | Doporučení |
|---|---|---|---|
| E1 | S | **Majetek ukazuje nuly místo načítání.** 5–10 s stojí „0 pozic“, „0 emisí“, „0 položek“ vedle správných částek a chybí sekce Nemovitosti; na Akciích „Sledované · 0“, pak 47. Viděno třikrát, ve třech jazycích. | Dokud dotaz neskončil, kostra místo čísla a místo sekce. Nula jen jako výsledek. |
| E2 | S | **Detail pozice zůstal přes 30 s na kostře** bez názvu, bez chyby a bez „Zkusit znovu“; po návratu a novém otevření se ukázal hned. Příčinu jsem neurčil. | Název a hodnotu vzít ze seznamu okamžitě, zbytek dočítat. Po časovém limitu `LoadError` s opakováním. |
| E3 | S | **Uložení jazyka nebo měny nemá viditelný stav.** Sheet zůstane otevřený, další klepnutí se tiše ignorují (`aria-busy` je jediný signál). V testu jednou viselo přes minutu; příčina je nejspíš v testovacím prostředí, chybějící zpětná vazba ne. | Kolečko u vybraného řádku, ostatní zšednout, po limitu toast s chybou. |
| E4 | N | Po odemčení je Přehled několik sekund celý v kostře, i když čisté jmění je jedno číslo z jednoho dotazu. | Hero vykreslit hned z posledního známého stavu, zbytek dočítat. |

---

## F. Pořadí práce

1. **Měny:** A1, A2, A3, A4 a jedno psané pravidlo do design systému (nativní částka, ≈ převod,
   desetinná místa). Test: profil s hlavní měnou USD a korunovými účty.
2. **Zkracování:** C1 napříč, D3 (Údržba), D13, D23, D25.
3. **Cashflow řádky:** D18, D19, D21.
4. **Mezistavy:** E1, E2, E3.
5. **Jednotnost:** C2 (dlaždice), C3 a C4 (avatary a titulky transakcí), C5 a C6 (data a jednotky),
   C7 (patičky sheetů).
6. **Jazyky:** B1, B2, B3, B5, A7.
7. Zbytek N podle chuti; nejvíc je vidět D1, C9, C11, C12.

Po každém kroku projít stejné obrazovky v cs + CZK, en + USD a de + EUR při 360 a 393.

---

## G. Co audit nepokryl

- **Zařízení.** WKWebView, bezpečné okraje, klávesnice a nativní výběr data, Face ID, haptika,
  Android. Vše výše je z Chromia.
- **Velikost písma.** Dynamic Type 130 % jsem neprošel.
- **Jazyky es, it, nb, pl, sv, zh** jen z kódu (délky textů, plurály). Podle rešerše je nejdelší
  dolní lišta ve španělštině („Flujo de caja“) a nejdelší tlačítko Údržby ve švédštině
  („Säkerhetskopiera“); naživo neověřeno.
- **Obrazovky, které jsem naživo neotevřel:** formuláře přidání (účet, úvěr, pojistka, nákup,
  prodej), záznam zůstatků, import a fronta kategorizace, Krypto, Dluhopisy, Ostatní aktiva, detail
  nemovitosti, Sledované, první spuštění, obnova ze zálohy, zbytek Nastavení pod první úrovní.
- **Zápisy.** Kromě jazyka a hlavní měny jsem v kopii nic neměnil.
