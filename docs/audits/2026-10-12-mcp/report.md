# Audit konzistence MCP zápisů s aplikací Moony

Datum: 12. října 2026. Kontrolovaný backend odpovídá commitu `953abceed97d108fa170148d8bf42ae34d84699b`; během auditu přibyly dva nesouvisející UI commity, výsledný HEAD je `f5dfe8fa467a00d238777cb342601b1b03a515ad`. Audit implementaci neopravuje. Přidané soubory obsahují zprávu a reprodukční podklady.

## Závěr

**Obava je oprávněná. MCP zatím nezaručuje stejnou konzistenci jako zadání přes aplikaci.** Bankovní evidence importů je opravená, ale obdobná mezera zůstává u akcií. Podstatnější jsou případy, kdy platně přijatý požadavek vytvoří rozporné množství, nesprávnou historii ocenění nebo vyřadí cashflow report.

Nalezeno **15 konkrétních problémů**: 5 s prioritou P1 a 10 s prioritou P2. P1 zde znamená opravit před spoléháním na MCP pro běžné hromadné importy; P2 znamená podmíněnou chybu konzistence, ztrátu možnosti nápravy nebo funkční rozdíl. Nejde o tvrzení, že všechny problémy vzniknou při každém importu.

Sdílení služeb je dobrý základ, ale samo o sobě nestačí. UI někdy doplňuje hodnoty, které MCP neposílá; některé importní služby vynechávají kontroly ručního zápisu; schémata nástrojů nejsou vždy v souladu s doménovým modelem; přepočty a navazující akce mají rozdílné spouštěče.

## Rozsah a metoda

Kontrola zahrnula všechny MCP finanční zápisy a jejich argumenty, odpovídající Tauri commands, sdílené služby, modelové validace, deduplikaci, databázové vazby a importní dávky, cenové identity, výpočty množství/nákladů, návaznost na historii a frontendový listener. Zahrnuty byly banky, akcie, kryptoměny, ostatní aktiva, nemovitosti, úvěry, dluhopisy, pojištění a Stock Monitor. U čtecích nástrojů se kontrolovala zejména návaznost na společné výpočty a dopad chybných zápisů.

Použity byly statické průchody od vstupu po čtení výsledku a **22 dodatečných charakterizačních testů** na dočasné kopii zdrojů s minimálními in-memory SQLite databázemi. Jejich úspěch potvrzuje současné chování, včetně chyb; nejsou to akceptační testy opravené implementace. Jeden z testů zachycuje dokumentované omezení identických obchodů a další potvrzuje chybějící ochranu opakovaného založení pojistky. Pro časové hranice a neplatné datum byl navíc spuštěn JavaScriptový reprodukční příklad.

Nebyly otevírány osobní databáze, prováděny importy do běžící aplikace ani volána externí cenová API. U problému s CoinGecko je vykonaným testem prokázán rozpor identity uložené v DB a identity v eventu; následné zapsání nesprávných cen je ověřeno průchodem zdrojů, nikoli živým stažením cen. UI render se u data ověřoval přes stejnou operaci `Intl.DateTimeFormat`, nikoli automatizací celé aplikace.

## Přehled nálezů

| ID | Priorita | Problém | Rozsah |
|---|---|---|---|
| F01 | P1 | Prodeje nad dostupné množství, rozporné výsledné množství | MCP akcie/krypto oproti UI |
| F02 | P1 | Počáteční nákup ostatního aktiva ponechá množství 0 | MCP při doporučeném vstupu; služba spoléhá na UI |
| F03 | P1 | CoinGecko ID z požadavku může přepsat historii jiné mince | MCP krypto |
| F04 | P1 | Spořicí účet bez volitelného úroku vyřadí cashflow report | MCP při běžném vstupu; chyba sdíleného modelu/čtení |
| F05 | P1 | `NaN`/`inf` projdou validací a uloží se do finančních faktů | Sdílené validace, snadno dosažitelné MCP |
| F06 | P2 | Akciový MCP import nemá dávku ani hromadné vrácení importu | MCP oproti akciovému CSV wizardu |
| F07 | P2 | Jiný zápis stejného čísla obejde deduplikaci | MCP akcie/krypto/ostatní aktiva |
| F08 | P2 | Deduplikace ignoruje měnu a může zahodit jiný obchod | Sdílená investiční deduplikace |
| F09 | P2 | MCP nabízí chybný typ nemovitosti | MCP schéma + nedostatečná sdílená validace |
| F10 | P2 | Ostatní aktiva sčítají nákupní ceny různých měn bez převodu | Sdílený výpočet, UI i MCP |
| F11 | P2 | Přijatá velikost písmen mění finanční význam | Účty a pojištění; UI běžně posílá kanonické hodnoty |
| F12 | P2 | Bankovní MCP import nespustí doplnění historických FX kurzů | MCP oproti bankovnímu UI |
| F13 | P2 | Lokální půlnoc potlačí potřebný historický přepočet | MCP event bridge |
| F14 | P2 | Neplatná/nekanonická data se uloží a rozbijí zobrazení/dedup | Sdílené validace, přímo dostupné MCP |
| F15 | P2 | Krypto MCP odmítá více měn, které UI podporuje | Rozdílné zapisovací služby |

## F01 — Prodeje nad držené množství

**Reprodukce:** přes `stock_transactions_create` nebo `crypto_transactions_create` zapsat v rostoucích dnech nákup 1 kusu, prodej 2 kusů a nákup 3 kusů. Všechny řádky se přijmou. U akcií test prokazuje uložené množství `2`, zatímco `cost_basis::replayed_quantity` vrátí `3`, protože nepřípustný prodej ořízne. Vznikne rozpor mezi uloženými souhrny a množstvím použitým při ocenění.

Importní větve volají interní INSERT bez `ensure_sell_within_holdings`: [akcie](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/investments.rs:455), [krypto](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/crypto_investments.rs:355). Ruční zápis tuto kontrolu má: [akcie](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/investments.rs:765), [krypto](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/crypto_investments.rs:251). Akciový CSV wizard navíc simuluje obchody před zápisem.

**Náprava:** sjednotit kontrolu v doménové zapisovací službě. Ověřit časově seřazenou kombinaci existujících a nových obchodů, včetně pozdějších již uložených prodejů; samotná kontrola množství v den nového prodeje nemusí zachytit dopad zpětně vloženého obchodu. Selhání musí vrátit celou MCP dávku.

## F02 — Počáteční nákup ostatního aktiva nevytvoří odpovídající držbu

**Reprodukce:** `other_asset_create` s `asset.name`, `asset.marketPrice` a `initialTransaction` typu buy, quantity `1`, price `10`; vynechat `asset.quantity` a `asset.averagePurchasePrice`, jak doporučuje popis nástroje. Transakce vznikne, ale vrácené i uložené množství a průměrná cena zůstávají `0`. Čisté jmění používá uložené množství, takže aktivum má nulovou hodnotu navzdory evidovanému nákupu.

Příčina je [zápis počáteční transakce bez přepočtu před commitem](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/other_assets.rs:125). UI problém obchází tím, že [počáteční množství a cenu předvyplní](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src/components/other-assets/AddOtherAssetModal.tsx:98). Existující testy služby toto historické chování dokonce zachovávají; nejde proto jen o chybějící test.

**Náprava:** při počáteční transakci odvodit a uložit souhrny uvnitř stejné SQL transakce. Nevyžadovat, aby klient poslal dvakrát vzájemně konzistentní finanční údaje.

## F03 — Historie kryptoměny používá neautoritativní CoinGecko ID

**Reprodukce:** v DB existuje BTC s ID `bitcoin`. Importovat další zpětně datovaný nákup BTC s `coingeckoId: "ethereum"`. Služba ponechá identitu držby `bitcoin`, ale event předá `ethereum`. Jiná varianta: dva řádky nového tickeru mají rozdílná ID a odlišné pořadí vstupu/dnů; seřazený zápis a výběr ID pro event zvolí jiný řádek.

Event vybírá ID [z původních argumentů](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/mcp/crypto.rs:308). Přepočet [důvěřuje dodanému ID](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/commands/portfolio.rs:2075), sestaví [mapu tohoto ID na původní ticker](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/commands/portfolio.rs:1718) a následně ukládá jeho ceny pod tento ticker do historie. Při dostupném cenovém API se tak historické ocenění BTC může odvozovat od ETH, přestože aktuální držba má správnou identitu.

**Náprava:** odmítnout konfliktní ID vůči existující držbě i uvnitř dávky. Rozsah přepočtu a cenovou identitu odvozovat z úspěšně zapsaných dat, nikoli z původního požadavku.

## F04 — Běžné založení spořicího účtu rozbije cashflow plán

**Reprodukce:** `bank_account_create({name:"Savings", accountType:"savings"})`. `interestRate` je volitelný, ale jeho vynechání uloží SQL NULL. Cashflow plán [čte tento sloupec jako povinný String a propaguje chybu](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/cashflow_planning.rs:482). Selže celý plán/report, i když nový účet má nulový zůstatek.

[MCP předává chybějící hodnotu](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/mcp/bank_accounts.rs:351), [sdílená služba ji zapisuje](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/bank_accounts.rs:114). UI obvykle posílá `"0"`. Reprodukční test nejprve ověřil fungující report, poté selhání po založení účtu a obnovení po změně jediného sloupce NULL → `0`.

**Náprava:** definovat výchozí sazbu ve sdílené službě a čtení existujících NULL hodnot ošetřit podle doménového významu. Oprava musí pokrýt i již vzniklé účty.

## F05 — Nečíselné konečné hodnoty nejsou odmítnuty

**Reprodukce:** nákup akcie/krypta/ostatního aktiva s quantity `"NaN"`; u krypta také pricePerUnit `"inf"`. Validace i import projdou a finanční sloupce mohou obsahovat `NaN`/nekonečno; u držby je prokázáno uložené množství `NaN`.

Validace parsuje `f64` a používá pouze porovnání s nulou: [akcie](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/models/investments.rs:353), [krypto](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/models/crypto.rs:192), [ostatní aktiva](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/models/other_assets.rs:179). `NaN <= 0` je false. Výpočty pak mohou vracet nedefinované hodnoty nebo je při čtení potlačovat; úspěšný zápis tak nezaručuje použitelný finanční fakt. Obdobné nedostatky jsou při statické kontrole vidět u některých částek dluhopisů, nemovitostí a pojištění. Bankovní transakce a relevantní validace úvěrů již konečnost kontrolují.

**Náprava:** centrální pravidlo pro číselné finanční vstupy, kontrola `is_finite()` před rozsahem, včetně přetečení zápisů typu `1e999`. Vedle vstupu jednotlivých hodnot prověřit i přetečení jejich součinů/součtů. Doplnit negativní testy a opravu zachovat ve sdílených validátorech.

## F06 — U akcií zůstává stejný problém s historií importů jako dříve u bank

**Reprodukce:** import jedné či více akciových transakcí přes MCP vytvoří 0 řádků v `stock_import_batches` a nové transakce mají `import_batch_id = NULL`. V historii akciových importů není co vrátit.

MCP volá [sdílený bulk writer s parametrem dávky `None`](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/mcp/investments.rs:283); CSV wizard předává metadata dávky. Akciová infrastruktura pro sledování a undo tedy existuje, ale MCP ji nevyužívá.

**Náprava:** stejně jako u bank vytvořit dávku pro MCP import, propojit nové řádky a umožnit bezpečné vrácení přes stávající historii. V API vracet identitu dávky a promítnout změnu do invalidací. U již importovaných nepropojených obchodů nepřiřazovat původ odhadem.

## F07 — Deduplikace rozlišuje zápis čísla místo jeho hodnoty

**Reprodukce:** import nákupu s quantity `"10"`, price `"100"`; opakování téhož s `"10.0"`, `"100.00"`. Oba zápisy projdou a množství vzroste na `20`. Potvrzeno testem akcií i krypta; ostatní aktiva používají stejný helper.

[SQL porovnává peněžní/množstevní TEXT přímo](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/dedup.rs:246). LLM může při dalším parsování exportu změnit formát bez změny hodnoty. Tvrzení, že překrývající se dávky jsou bezpečné díky deduplikaci, tak platí jen při stejném textovém formátu. Bankovní deduplikace již numerické zápisy kanonizuje.

**Náprava:** přesná kanonizace desetinných hodnot při porovnávání a ukládání, ideálně doplněná identitou obchodu od zdroje. Nekanonizovat přes ztrátový float tak, aby splynuly odlišné přesné hodnoty.

## F08 — Deduplikace opomíjí měnu

**Reprodukce A:** ostatní aktivum, stejný den, množství 1, cena 10; jeden nákup v EUR, druhý v CZK. MCP import přijme jen jeden a druhý označí za duplicitu, přestože jde o odlišný finanční fakt.

**Reprodukce B:** již existující AAPL USD nákup; dávka obsahuje stejná čísla a datum v EUR plus další platný USD obchod. EUR řádek je přeskočen jako duplicita ještě před kontrolou měny, druhý řádek se uloží. Dávka tedy neodmítne měnový rozpor, přestože to deklaruje popis nástroje.

Příčina je opět [klíč bez měny](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/dedup.rs:246) a pořadí deduplikace vůči byznysové validaci.

**Náprava:** zahrnout kanonickou měnu do identity a ověřit měnovou kompatibilitu před přeskočením řádku. U domény, která podporuje více měn, musí oba odlišné obchody zůstat zachované.

## F09 — Schéma nemovitosti navádí k neplatné klasifikaci

MCP schéma [uvádí `apartment`, `house`, `land`](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/mcp/real_estate.rs:114), zatímco aplikace pro toto pole používá `personal` / `investment`. Sdílená validace přijímá libovolný neprázdný řetězec.

**Reprodukce:** vlastní bydlení s `type: "house"` se uloží. [Čisté jmění](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/net_worth.rs:121) jej zařadí mezi investice, protože není přesně `personal`. [Cashflow](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/cashflow_planning.rs:162) jej naopak považuje za osobní, protože není přesně `investment`. Volba vyloučit vlastní bydlení z majetku jej nevyloučí.

**Náprava:** popsat a vynutit kanonický enum ve schématu i sdíleném modelu; opravit příklady. Stávající neplatné hodnoty nelze vždy automaticky převést, protože `house` neříká, zda je dům investiční nebo osobní.

## F10 — Průměrná pořizovací cena ostatních aktiv ignoruje měny

**Reprodukce:** aktivum v EUR, jeden kus za 10 EUR a druhý za 100 CZK. Uložená průměrná pořizovací cena je `55 EUR`; při kurzu 25 CZK/EUR by správně vyšla `7 EUR`.

[Výpočet nečte měnu transakce a sčítá ceny bez konverze](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/other_assets.rs:17). Jde o sdílenou chybu, dosažitelnou UI i MCP. Týká se uloženého průměru a jeho spotřebitelů, například výnosu `percent_purchase`; frontend některé celkové náklady odvozuje zvlášť z historie, takže ne každý údaj na stránce musí být chybný stejně.

**Náprava:** buď transakční měnu omezit na měnu aktiva, nebo pořizovací cenu odvozovat pomocí historických převodů přes `services/fx.rs` a explicitně zacházet s chybějícími kurzy. Nikdy nesčítat holá čísla různých měn.

## F11 — Validace toleruje velikost písmen, persistence a reporty nikoli

**Reprodukce A:** účet `accountType: "SAVINGS"`, balance `1000`, interestRate `5` projde a uloží se v původní podobě. Cashflow hledá pouze `account_type = 'savings'`, takže úrok chybí. Celý report testem projde, ale příjem je vynechán; změna pouze velikosti písmen jej vrátí.

**Reprodukce B:** pojištění se `status: "ACTIVE"` projde [case-insensitive validací](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/models/insurance.rs:197), uloží se doslova a [cashflow vybere pouze `status = 'active'`](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/cashflow_planning.rs:325). Pojistné je vynecháno.

**Náprava:** enumové hodnoty kanonizovat ve společných create/update službách, ideálně pomocí typovaných enumů. UI obvykle posílá kanonickou podobu, proto jde o typický problém při přímém importu přes API.

## F12 — Bankovní MCP import nedoplňuje historické směnné kurzy

UI po [ruční transakci](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/commands/bank_accounts.rs:319) i [CSV importu](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/commands/bank_accounts.rs:538) spouští `spawn_fx_backfill`. MCP bankovní zápis tuto návaznost nemá. Jeho [frontendový listener](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src/hooks/SyncProvider.tsx:167) obnoví dotazy a snapshot, ale nedoplní kurzovou historii.

**Scénář:** kurzová historie pokrývá současnost; přes MCP se přidá starý cizoměnový výpis. Historický report nemá potřebné kurzy a částky vynechá s informací o chybějícím FX. Stejný import přes UI automaticky zkusí historii doplnit. Samotný refresh dotazů chybějící kurzy nezíská; napraví se až jiným spouštěčem doplnění, například dalším refreshem FX.

**Náprava:** doplnění FX spouštět po úspěšném zápisu z obou vstupů společným backendovým mechanismem. Zachovat best-effort charakter: síťové selhání nesmí zneplatnit úspěšný import. Tento nález je ověřen statickým průchodem spouštěčů; živé stažení FX nebylo součástí auditu.

## F13 — Půlnoc v lokální zóně není stejná jako backendový UTC den

[Listener používá lokální půlnoc](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src/hooks/SyncProvider.tsx:234), ačkoli backend přepočítává po UTC dnech. Komentář v kódu chybně tvrdí, že rozdíl může pouze rozšířit okno.

**Reprodukce:** zóna Europe/Prague, aktuální datum 12. 10. 2026, importní datum `2026-10-11T23:00:00Z`. Lokální hranice dne je `2026-10-11T22:00:00Z`; frontend vrátí `isRetroactive = false`. Backendová hranice je `2026-10-12T00:00:00Z`, takže přepočet je potřeba. MCP aktuálně intradenní epochy přijímá. Pro kanonické UTC půlnoci se tento konkrétní scénář neprojeví; oba problémy se musí řešit společně s F14.

**Náprava:** UTC hranice ze společného helperu; neduplikovat rozhodnutí backendu v klientovi. Doplnit testy pro UTC+ i UTC− zóny.

## F14 — Validace kalendářních dat dovolí neplatné a nekanonické epochy

**Reprodukce:** MCP bankovní transakce s `bookingDate: 9223372036854775807` nebo stejnou `valueDate` projde. U bookingDate se kontroluje pouze kladnost, valueDate není kontrolována. Extrémní datum se přenese i do časového rozsahu importu. [Formátování](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src/lib/format.ts:155) pak vyvolá `RangeError: Invalid time value`; postižen je seznam, detail nebo historie importů podle toho, které pole je neplatné.

[Validace](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/models/bank_transactions.rs:213) také nevynucuje UTC půlnoc požadovanou ADR 0008. Běžnější scénář než extrémní číslo: tatáž bez-ID transakce importovaná z CSV s půlnocí a poté přes MCP s polednem stejného dne může obejít přesný datumový klíč deduplikace. Investiční datumy rovněž vyžadují sjednocení kontraktu; současné popisy často uvádějí jen „Unix seconds“.

**Náprava:** validační/normalizační pravidlo pro všechny kalendářní datumy, podporovaný rozsah, UTC den a explicitní práci s volitelnými datumy. Bezpečně zobrazit již uložené neplatné hodnoty, aby se uživatel mohl dostat k jejich odstranění. Při normalizaci starších dat zachovat identitu finančních faktů.

## F15 — Krypto UI a MCP mají rozdílnou politiku transakčních měn

**Reprodukce:** po nákupu BTC v USD provést nákup BTC v EUR. Ruční `add_transaction_to_crypto` jej přijme, MCP import [odmítne rozdíl proti měně držby](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/crypto_investments.rs:327). Reprodukční test volal obě služby na ekvivalentních DB a potvrdil rozdíl.

Současný `cost_basis` přitom výslovně podporuje transakce krypta ve více měnách s historickým přepočtem. Nejde o tiché poškození při odmítnutí: MCP odmítne celou dávku. Jde o funkční nesoulad, který komplikuje import burzovních exportů a může uživatele vést k chybnému přeznačení měny.

**Náprava:** jednu měnovou politiku ve společné doménové službě. Pokud je cílem více měn, využít podporovaný převod; pokud ne, musí stejné omezení dostat UI a změna musí být výslovně rozhodnuta.

## Další omezení a menší nálezy

- **Dva skutečně identické obchody:** akcie/krypto/ostatní aktiva je považují za duplicitu i uvnitř jednoho exportu. Je to popsané chování, nikoli překvapivá odchylka implementace, ale pro dva shodné burzovní filly může být nedostatečné. MCP akciové schéma navíc nepropouští existující `external_id` ani `allow_duplicate`; banky mají occurrence matching a potvrzovaný `importAnyway`. Doporučení: zdrojové ID a možnost rozlišit skutečné opakování bez změny finančních hodnot.
- **Opakované založení pojištění:** stejný provider + policyNumber lze importovat znovu a vzniknou dvě pojistky i dvojnásobné plánované pojistné. Potvrzeno testem. Nástroj neslibuje idempotenci a UI rovněž nemá ochranu; jde o riziko opakování po timeoutu a rozdíl vůči MCP create nástrojům, které kontrolují duplicity. Doporučení: definovat identitu pojistky a způsob práce s opakovaným požadavkem, ne plošně zakázat dvě legitimní podobné pojistky.
- **Stock Monitor mění data i při chybném požadavku:** `watchlist_set_target_price` [nejdříve založí sledování a teprve poté validuje cíl](/Users/filipkral/Programování/FinanceApp/Moony-tauri/src-tauri/src/services/mcp/stock_monitor.rs:165). Chybná cena/direction vrátí chybu, ale nový sledovaný ticker zůstane. Nižší dopad: nejde o finanční držbu. Validovat před zápisem nebo obě operace spojit atomicky.
- **Krypto a ostatní aktiva nemají stejnou infrastrukturu dávek jako akcie/banky.** Chybějící undo v těchto doménách je současné produktové omezení i na straně UI, nikoli další stejná odchylka jako F06.
- **Zůstatky bankovních účtů jsou samostatná fakta.** Import transakcí nemá automaticky přepsat bankovní zůstatek. CSV nabízí následné potvrzené zapsání statement balance; MCP nemá odpovídající nástroj pro nový záznam zůstatku na existujícím účtu. To omezuje úplnost importu, ale samo o sobě nejde o chybný součet transakcí.
- **Vedlejší účinky závisí na rendereru.** MCP commit je backendový, snapshot a řada přepočtů jsou best-effort přes event a frontend. Událost není trvalá fronta a výsledek nástroje nečeká na dokončení přepočtů. Pád/reload UI či okamžité zamčení může přerušit návaznosti; startup doplňuje chybějící dny, ale automaticky neopraví všechny již existující zastaralé snapshoty. Jde o architektonické riziko vyplývající z implementace, nikoli o další dynamicky reprodukovaný nález tohoto auditu.
- **Dokumentace obsahuje zastaralé pasáže.** ADR 0007 stále popisuje starší finální MCP kategorizaci, aktuální implementace používá návrhy k potvrzení. Při opravách se řídit aktuální implementací a kontrakty, ne kopírovat staré návrhy.

## Co je již správně a co není třeba znovu opravovat

1. Bankovní MCP import nyní skutečně vytváří záznam historie pro validní neprázdné volání, včetně duplicate-only volání. Nové transakce mají vlastníka a lze je smazat přes historii účtu.
2. Oba bankovní importy používají společné zápisy, dedup, pravidla kategorizace, pending promotion a batch helpers. Není zde druhá nezávislá sada INSERTů jako náhrada těchto služeb.
3. Promotion pending → booked ponechává původního vlastníka importu; smazání nové dávky tím nesmaže starší cizí řádek. Vlastní uživatelské zařazení a finanční význam se chrání.
4. MCP bankovní kategorizace ukládá návrhy a nepřepisuje existující finální kategorie. Potvrzení návrhu není trénování pravidla z odhadu LLM.
5. Bulk limity 200 řádků jsou kontrolované i zveřejněné ve schématu. Dávky používají SQL transakce a otestované rollbacky. To však neřeší pravidla, která validace vůbec neobsahuje, například F01.
6. CSV bankovní import záměrně toleruje vadné řádky, MCP záměrně odmítá celou chybnou dávku. Atomická je jednotlivá MCP call, nikoli několik chunků celého souboru.
7. Založení nemovitosti a ostatního aktiva zahrnuje úvodní valuaci atomicky. Úvěrové anchory procházejí sdílenou validací; neexistence eventu při počátečním anchoru je záměr.
8. Při vynechaném CoinGecko ID existující držby přepočet umí ID načíst z DB. Chyba F03 nastává u dodaného konfliktního ID.
9. Portfolio, pozice a hlavní reporty používají sdílené služby a explicitní měny. Chybějící FX se deklaruje a neoceňuje 1:1.
10. Server se váže na localhost, bearer token se kontroluje na HTTP vstupu a je uložen uvnitř šifrované DB. Audit nenalezl nutnost otevírat další zapisovací cestu mimo služby. Nejde však o plný penetrační audit transportu.

## Doporučené pořadí práce

1. **Nejdříve integrita faktů a výpočtů:** F01–F05. U oprav převést charakterizační testy na testy požadovaného chování a doplnit i variantu přes UI. Pro opravu neplatných existujících dat vytvořit samostatný postup s potvrzením uživatele tam, kde záměr nelze bezpečně odvodit.
2. **Poté bezpečnost importu a jeho vrácení:** F06–F08, jednotná zdrojová identita a přesná numerická kanonizace. Zabránit falešnému přeskočení i nechtěnému zdvojení.
3. **Sjednotit doménové kontrakty:** F09–F11, F14–F15. Enumy, měny a datumy vynutit na společné vstupní hranici; frontend nemá být jediným místem, které je drží konzistentní.
4. **Sjednotit následné akce:** F12–F13 a backendová orchestrace po commitu. Zápis může zůstat rychlý; síťové doplnění a přepočet mohou běžet asynchronně, ale jejich potřeba by neměla zaniknout s frontendovým eventem.
5. **Trvalý test parity:** pro stejné vstupy porovnávat finanční fakty, souhrny, vlastnictví importu, odvozené množství a potřebné následné úlohy napříč UI/MCP službami. Netestovat jen počet vytvořených řádků. Současná zelená sada zachycené mezery nevylučuje.

## Ověření a reprodukční podklady

| Kontrola | Výsledek |
|---|---|
| `npm test` | 93 souborů, 1467 testů prošlo |
| `npm run typecheck` | Prošlo |
| `cargo test --offline` na původních zdrojích | 1660 library testů + 7 pomocného převodníku + 10 seed tool + 1 doctest prošlo; 5 testů vynecháno dle jejich konfigurace |
| Dočasná kopie s dodatečnými testy | 22 dodatečných testů prošlo a potvrdilo auditované chování |
| JavaScript, `TZ=Europe/Prague` | Potvrzeno lokální/UTC okno a `RangeError` pro extrémní datum |

První sandboxový běh dvou HTTP testů neprošel kvůli zakázanému localhost bindu. Samostatné i následné úplné spuštění s povoleným loopbackem prošlo. Není to zjištěná chyba MCP ani zamlčené selhání testů. Lint a produkční build nebyly pro tento audit bez změny implementace spouštěny.

[Spouštěč reprodukcí](/Users/filipkral/Programování/FinanceApp/Moony-tauri/docs/audits/2026-10-12-mcp/reproduce.py) kopíruje zdroje do dočasného adresáře, přidá proof moduly a spustí tři skupiny testů. Používá samostatný Cargo target, neotevírá SQLCipher profil a po skončení odstraní dočasné zdroje. Vyžaduje dostupný Rust toolchain a již stažené Cargo závislosti; první sestavení může trvat déle. Sestavení testů a jejich spuštění bylo v auditu provedeno; výsledná pomocná varianta spouštěče byla následně zkontrolována syntakticky a vůči vykonanému postupu.

Podklady: [banky](/Users/filipkral/Programování/FinanceApp/Moony-tauri/docs/audits/2026-10-12-mcp/proofs/bank.rs), [akcie](/Users/filipkral/Programování/FinanceApp/Moony-tauri/docs/audits/2026-10-12-mcp/proofs/stocks.rs), [krypto](/Users/filipkral/Programování/FinanceApp/Moony-tauri/docs/audits/2026-10-12-mcp/proofs/crypto.rs), [ostatní aktiva a entity](/Users/filipkral/Programování/FinanceApp/Moony-tauri/docs/audits/2026-10-12-mcp/proofs/assets.rs). Při opravách se očekávané výsledky musí změnit: například oversell má být odmítnut, nikoli přijímán jen proto, aby charakterizační test dál procházel.
