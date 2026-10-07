# Stav nápravy auditu toků peněz

Datum: 7. října 2026. Větev: `codex/money-flows-readiness`; výchozí commit `cbf1494`.

Změny popsané níže jsou implementované a prošly testy i samostatnými review jednotlivých úloh. Závěrečné průřezové review celé větve ještě probíhá. Tento dokument navazuje na [výchozí audit](2026-10-06-money-flows-audit.md), [návrh](../specs/2026-10-06-money-flows-readiness-design.md) a [plán](../plans/2026-10-06-money-flows-readiness.md).

## Co nyní znamenají čísla

Bankovní účet ukazuje příchozí a odchozí pohyby v jeho měně. Rozpočty a skutečné cashflow pracují se zaúčtovanými pohyby a jejich finančním významem. Vratka snižuje výdaje, storno snižuje příjmy. Převody mezi vlastními účty, investice, financování a úpravy jsou oddělené od běžných příjmů a výdajů. Celkový pohyb peněz na účtech je proto zobrazen samostatně od běžného příjmu po odečtení výdajů.

Ručně stanovený význam transakce má přednost před identitou vlastního účtu, ta před rolí kategorie a nakonec rozhoduje směr pohybu. Rozhraní používá výsledek stejného serverového resolveru. Při chybě nebo chybějícím výsledku zobrazí neurčený význam a opakování načtení; nevymýšlí příjem či výdaj podle směru.

Historické součty používají doložený kurz k datu zaúčtování do interní CZK. Přepočet součtu do zvolené zobrazovací měny je označen aktuálním dostupným kurzem. Bez něj zůstávají hodnoty výslovně v CZK. Chybějící historický kurz znamená vynechaný příspěvek a neúplný součet, nikoli převod 1:1. Původní částku a měnu lze stále dohledat v detailu.

## Opravy nálezů A01–A19

| Nález | Výsledné chování | Důkaz |
| --- | --- | --- |
| A01 vlastní převody | Jistý vlastní převod nepřebije obchodní ani naučené pravidlo; explicitní význam má přednost. Nekonzumní toky jsou oddělené. | Regrese resolveru a engine; samostatné skupiny a význam v náhledu. |
| A02 identita účtu | Číslo účtu, kód banky, země a původní identifikátor se zachovávají. Úplné české číslo odpovídá platnému IBAN. Neúplné nebo známé zahraniční údaje se nepovažují automaticky za jistou českou shodu. | Testy identity, Fio kódu a národních délek; viditelné číslo `19-2000145399/0800`. |
| A03 duplicity | Zachovává se počet legitimních stejných plateb bez ID. Opětovný a překrývající se import porovnává uložené výskyty. Jisté bankovní ID nelze vynutit jako další kopii. | Testy multiplicity, měny, protistrany, desetinného TEXT a starších ID s mezerami; možné/jisté ovládání v náhledu. |
| A04 vratky | Výdaj 1 000 a vratka 500 dávají výdaje 500 v obou přehledech. Záporné i nulové skupiny s transakcemi zůstávají dostupné. Podíly používají absolutní hodnoty součtů skupin. | Regrese služeb; skupiny 1 000 a −500, signed detail a nulová nezařazená skupina. |
| A05 příjem a storno | Příjem 1 000, storno 100 a vlastní příjmová kategorie 2 000 dávají příjmy 2 900. Kategorie má explicitní roli; pohyb a význam jsou rozlišeny. | Regrese společné interpretace; přehled a formulář role. |
| A06 měny | Nativní souhrn a graf účtu zahrnují pouze zaúčtované pohyby ve stejné měně. Vynechané jiné měny jsou uvedeny. Reporty nepoužívají neznámý kurz jako 1:1. | Utility a služby; chybějící AED i nepodporovaná zobrazovací měna v náhledu. |
| A07 obnova pohledů | Zápisy čekají na obnovu domén, reportů, cílů, detailů a portfolia, poté pořídí snapshot a obnoví historii. Změny bankovních dat přes MCP obnovují i vlastní identity engine. | Centrální helper a review volajících cest; neúspěšná kategorizace nespustí učení. |
| A08 zůstatek | Výpis nabízí částku s měnou, datem, řádkem a dávkou. Starší nebo cizoměnový zůstatek nelze použít; neznámé současné datum vyžaduje výslovné potvrzení. Graf je označen jako odhad. | Guard a atomické služby; potvrzení a payload známého/neznámého data, blokace staršího i cizoměnového výpisu. |
| A09 neplatné částky | Účty, transakce, cíle a plánování odmítají nečíselné a nekonečné částky. Původní peněžní hodnoty zůstávají TEXT. | Validační regrese a typový kontrakt. |
| A10 kurzy | Chybějící historický kurz je uveden počtem a měnou vedle součtů. Detail ukazuje původní pohyb a nedostupný příspěvek. Odhady mají kvalitu a datum. | Testy FX a chyb SQL; AED detail bez přepočtu a viditelně neúplný report. |
| A11 dlouhá historie | Historie používá serverové filtry a stránky; úplné souhrny čtou omezené stránky až do konce. CSV má dostupné všechny stránky a globální volby podle fyzického řádku. | Služba s 1 213 řádky; vyhledání řádku za hranicí 1 000; import 405 řádků a zachovaná volba řádku 406. |
| A12 chyby | Reporty, průměry a drilldowny nabízejí chybu a opakování načtení. Chyba výpočtu významu nevytváří falešné označení. | Propagace SQL chyb, review UI a řízená chyba reportu/resolveru. |
| A13 historický limit | Verze limitů platí od efektivního měsíce; změna ani odstranění cíle nepřepisuje starší období. Editace zachovává nezměněnou částku a používá aktuální cíl, nikoli historické UUID. | Testy verzí a nulové útraty; přesný payload `1000.123` a UTC efektivní měsíc. |
| A14 průměry | Vycházejí jen z celých pozorovaných měsíců s dostupným přepočtem. Částečné měsíce a mezery jsou uvedeny. Bez vzorku zůstává průměr nedostupný. | Regrese výběru měsíců; 0/1 pozorovaný měsíc v náhledu bez tvrzení o úplnosti výpisů. |
| A15 období | Částečné srovnávací intervaly jsou symetrické; YTD používá kalendářní rok a ošetřuje přestupný den. Booking days i hranice rozpočtů používají UTC kalendář. | Regrese intervalů, UTC utilit a přestupného roku. |
| A16 učení | Učení je standardně vypnuté. Nové pravidlo je omezeno účtem, směrem a spolehlivou identitou nebo normalizovaným názvem. Databázový commit předchází změně paměti; starší široká pravidla jsou označena. | Scoped regrese restartu; vynucená chyba INSERT po DELETE zachová DB i engine; chybný zápis kategorie neučí. |
| A17 plánování | Splacené/neaktivní úvěry se nezapočítávají. Ruční položka může nahradit konkrétní automatický zdroj. Měsíční ekvivalent, původní frekvence, datum a kvalita odhadu jsou uvedeny. | Regrese půjček, náhrad a FX; ročních 6 000 odpovídá 500 měsíčně a editace uchová 6 000/ročně. |
| A18 valuta a původ | Volitelné datum valuty, bankovní ID, soubor, řádek, dávka a zůstatek řádku jsou dostupné v detailu. Mapování vysvětluje účel jednotlivých sloupců. | Importní regrese a detail transakce v náhledu. |
| A19 čekající platby | Čekající pohyby jsou označené a nezahrnují se do skutečných reportů a souhrnů. Stejné ID se bezpečně aktualizuje na zaúčtovaný pohyb se zachovaným původním vlastnictvím a ručním významem. | Testy whitelistu, promotion a preview/import shody; badge a oddělený počet aktualizovaných pohybů. |

Importní historie nyní rozlišuje „Smazat importované transakce…“ a „Smazat záznam importu“ u dávky bez vlastních transakcí. Potvrzení uvádí aktuální připojený počet, soubor a období. Atomická operace vrací skutečně odstraněný počet; jiné importy, samostatně změněný zůstatek a pravidla zůstávají zachovány. Starší transakce se nepřiřazují dávce odhadem.

## Ověření a review

- Frontend: 983 testů v 72 souborech; lint, typecheck, Prettier a build prošly. Vitest po opravě končí bez původního Vite close timeoutu.
- Rust: 1 452 knihovních + 7 převodních + 10 seed + 1 dokumentační test = 1 470 úspěšných testů; 4 generátory/ručně spouštěné kontroly jsou v běžném běhu ignorované. Formát, cargo check a Clippy všech targetů s `-D warnings` prošly.
- Generování typů a snapshotu schématu prošlo bez změny oproti uloženým souborům.
- Testy služeb používají samostatné in-memory SQLite Connection. Úplný Rust běh měl povolené pouze potřebné lokální HTTP fixture porty.
- Úlohy 1–5 prošly nezávislým review. Nalezené chyby FX kvality a SQL propagace, CSV statusů/promotion/ID, finančního významu a vynechaných skupin i popisků byly opraveny a znovu ověřeny.
- Závěrečné review celé větve: čeká na dokončení.

Průřezový běh byl proveden na `e8df0a2`; následná čistě jazyková oprava `34bf155` znovu prošla frontendovými testy, typecheckem, lintem, kontrolou formátu a locale klíčů. Rust se touto opravou nezměnil. Lokální surové logy jsou v `/tmp/moony-final-*.log`; trvalý doklad představují souhrny zde, regresní testy a uvedené commity.

## Hranice ověření a globální použití

Vizuální scénáře běžely v izolovaném frontendovém náhledu přes oficiální Tauri mockIPC s testovacími údaji. Ověřují spotřebu kontraktů a ovládání; správnost SQL a výpočtů dokládají samostatné regresní testy. Nainstalovaná aplikace se soukromou SQLCipher databází nebyla otevřena, upravena ani nahrazena. Nebyl proveden push, merge nebo produkční instalace.

Podporované kategorizační sady jsou zachovány. Finanční význam nezávisí na českém názvu kategorie ani na dvou pevných ID příjmů/převodů. Zahraniční identifikátory a nativní měny se zachovávají; chybějící kód banky se nevymýšlí. Nové učení nevytváří globální pravidlo po restartu. Interní CZK zůstává záměrnou architektonickou základní měnou; zvolená měna zobrazení a historický přepočet jsou rozlišeny.

Build stále uvádí starší upozornění na velké chunky a neúčinné dynamické importy. Nejde o neúspěšný build; reorganizace celého bundlu nebyla součástí této nápravy. Tento audit nepředstavuje produkční certifikaci ani ověření všech bankovních formátů na všech platformách.
