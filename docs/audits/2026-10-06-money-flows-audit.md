# Audit importu transakcí, kategorizace, rozpočtů a cashflow

Datum: 6. října 2026. Zdrojový kód: commit `cbf1494`, verze v package.json: 0.9.1.

## Verdikt

**Produkční spuštění této části aplikace doporučuji odložit do odstranění problémů s finanční interpretací, duplicitami a měnami.** Vizuální základ je čistý a navazuje na existující design systém. Největší problém je důvěryhodnost: tatáž transakce může mít jiný význam v detailu účtu, rozpočtu a cashflow. Rozhraní přitom čísla prezentuje jako definitivní.

Nejde o potřebu kompletně zahodit vzhled. Doporučuji především sjednotit model peněžních toků, vytvořit spolehlivou identitu účtů a doplnit dohledatelnost a vratnost akcí. Kosmetické úpravy patří až za tato rozhodnutí.

Nálezy níže rozlišují:
- **P1:** opravit před produkcí; ztráta skutečné transakce při importu, nesprávné částky nebo podstatné zkreslení.
- **P2:** opravit pro srozumitelnou první verzi; provozní, UX a plánovací problémy.
- **Ověřeno sondou:** chování reprodukované skutečnou službou na izolované SQLite databázi v paměti.
- **Ověřeno kódem/UI:** dohledaná implementace nebo pozorování aplikace; nejde o destruktivní zkoušku na uživatelských datech.
- **Neuzavřený případ:** konkrétní historický incident bez doloženého původu dotčených dat.

## Rozsah a omezení

Prošel jsem importní dialog, parsery, presety, deduplikaci, zápis a mazání transakcí, kategorizační waterfall a učení, rozpočtové výpočty a cíle, skutečné cashflow, plánované cashflow, obnovování klientských dat a související překlady. Ve spuštěném Moony jsem prohlédl bankovní účty, detail účtu, historii transakcí, rozpočty a oba pohledy cashflow.

Reálné transakce jsem nemazal ani nepřekategorizoval. Původní hlášený případ smazaného importu jsem proto nerozhodl analýzou soukromé databáze. Běžící instalace nebyla ověřena jako build přesně odpovídající uvedenému commitu; důkazy z UI a důkazy ze zdrojového kódu jsou oddělené. Audit není plný bezpečnostní audit, účetní certifikace ani ověření všech existujících bankovních exportů.

## Odpovědi na vaše tři postřehy

### „Vrátit zpět“ odstraní import, ale transakce zůstávají

Aktuální `delete_import_batch` explicitně maže transakce s odpovídajícím `import_batch_id` a pak importní záznam. Současný CSV zápis tento vztah ukládá. Sonda potvrdila odstranění transakcí vytvořených touto cestou.

Potvrdil jsem ale scénář, který působí jako vaše chyba: první soubor vytvoří transakci, druhý soubor ji vyhodnotí jako duplicitu a vytvoří import s nulou nových transakcí. Smazání druhého importu správně ponechá transakci z prvního. Další možné příčiny jsou starší data bez vazby na import nebo opětovný import stejné platby. **To jsou hypotézy pro váš konkrétní případ, nikoli potvrzená diagnóza.**

Samotná akce není úplné „undo“: nevrací samostatně změněný zůstatek, zapnutý balíček pravidel ani naučená pravidla. Počet v historii je počet původně vložených transakcí, nikoli současný počet po ručním mazání. Při obnově přehledu je možné zaměnit podobné platby z různých zdrojů.

Doporučení:
- Akci pojmenovat **„Odstranit transakce z importu…“** / **“Delete imported transactions…”**.
- Před potvrzením ukázat soubor, účet, období a **skutečný počet dosud existujících transakcí**.
- U importu s nulou vložených řádků nabídnout **„Odstranit záznam importu“**.
- V detailu transakce ukázat „Zdroj: výpis.csv, řádek 18“ a odkaz na import.
- Mazání provést v jedné SQL transakci, vrátit počet odstraněných položek a podpořit obnovu.
- Starší nenavázaná data nespojovat se souborem automaticky jen podle částky a data.

Důkaz: [mazání importu](../../src-tauri/src/commands/bank_accounts.rs#L649), [zápis importu](../../src-tauri/src/services/csv_import/mod.rs#L409), sonda `audit_delete_only_removes_rows_owned_by_that_batch`.

### Proč import nabízí debet, kredit, zůstatek a ID transakce?

| Údaj | Skutečný účel v současném kódu | Co má rozhraní říct |
|---|---|---|
| Debet | Odchozí strana exportu; při dvojici sloupců se počítá kredit − debet. | „Odchozí částka — některé banky exportují příchozí a odchozí platby zvlášť.“ |
| Kredit | Příchozí strana téže dvojice; výsledkem je jedna částka a směr transakce. | „Příchozí částka.“ |
| Zůstatek | Hodnota z chronologicky posledního vybraného řádku; používá se pouze pro nabídku aktualizace zůstatku účtu. Neukládá se ke každé transakci. | „Zůstatek po transakci — může pomoci ověřit výpis.“ |
| ID transakce | Bankovní identifikátor, uložený v transakci; pomáhá bezpečně rozpoznat opakovaný import. | „ID platby z banky — doporučeno pro kontrolu duplicit.“ |
| Měna | Měna dané transakce; bez sloupce se použije měna účtu. | Jasně ukázat použitou měnu a upozornit na více měn. |
| Protistrana / účet | Popis a vstup kategorizace, včetně rozpoznání vlastních převodů. | Rozlišovat jméno, číslo účtu a identifikátor banky. |

Samostatné sloupce debet/kredit v běžném seznamu transakcí nejsou potřebné. Technické ID patří do detailu. Zůstatek se naopak hodí pro kontrolu úplnosti importu; dnes taková kontrola chybí.

Pokud je vybrána jediná částka, má přednost před debetem/kreditem. Rozhraní dnes ukazuje všechny možnosti najednou. Doporučuji přepínač **„Jeden sloupec částky / Příchozí a odchozí zvlášť“**, který nabídne jen relevantní pole.

Důkaz: [výpočet částky](../../src-tauri/src/services/csv_import/rows.rs#L181), [význam importních polí](../../src-tauri/src/services/csv_import/types.rs#L48), [mapování](../../src/components/bank-accounts/csv-import/MappingStep.tsx#L176).

### Číslo účtu bez kódu banky

**Potvrzeno pro protistrany.** Fio fixture obsahuje zvlášť `Protiúčet` a `Kód banky`, preset však mapuje jen `Protiúčet` do pole pojmenovaného IBAN. Obdobně české presety používají domácí čísla v poli `counterpartyIban`. Samostatné mapování kódu banky neexistuje.

Zobrazení každý nečeský-IBANový vstup formátuje po čtyřech znacích. Tak se domácí číslo změní na text podobný IBANu, přestože kód banky v uložené hodnotě chybí. To jsem viděl i v běžícím seznamu transakcí. Nejde jen o estetiku: bez identifikátoru banky nelze spolehlivě rozlišit účty ani párovat vlastní převody.

U vlastního účtu se preferuje uložené `bban` před převodem z IBANu. Pokud obsahuje jen číslo bez banky, pohled jej sám nedoplní. U vašeho konkrétního vlastního účtu jsem obsah těchto polí neověřoval.

Doporučení: české číslo `123456789/0800` nebo `19-123456789/0800`; zahraniční IBAN skutečně jako IBAN; ostatní domácí formáty podle země. Kód banky nelze spolehlivě odhadnout z názvu protistrany.

Důkaz: [Fio preset](../../src-tauri/resources/csv-presets/fio.json), [fixture s kódem banky](../../src-tauri/tests/fixtures/csv/fio.csv), [zobrazení protistrany](../../src/pages/BankAccountDetail.tsx#L1252), [formátování](../../src/utils/iban-utils.ts).

## Nálezy, které brání důvěryhodnému spuštění

### A01 · P1 · Interní převod je pouze kategorie, nikoli samostatný význam transakce

Rozpoznání vlastních účtů používá IBANy z tabulky účtů. Domácí identifikátory, routing/sort/BSB kódy, čísla karet ani vazbu mezi dvěma transakcemi nezná. Naučený příjemce a vlastní pravidlo mají přednost před rozpoznáním vlastního IBANu. Sonda potvrdila, že naučené jméno s kategorií restaurace přebije prokazatelný vlastní IBAN.

Přidání účtu obnoví sadu IBANů v paměti, ale nepřezkoumá již zařazené transakce. Automatická kategorizace detailu zpracovává jen dosud nezařazené položky. Stejné jméno se navíc může vztahovat na více vašich účtů i na jiné osoby.

**Změna:** oddělit povahu toku od spotřební kategorie; vlastní převody identifikovat přes normalizovanou identitu účtu a případně propojený pár. Automatické pravidlo obchodníka nesmí nepozorovaně změnit povahu potvrzeného vlastního převodu. Explicitní zásah uživatele umožnit s vysvětlením. U známého vlastního cíle nemusí v aplikaci existovat obě strany převodu.

Důkaz: [waterfall](../../src-tauri/src/services/categorization/engine.rs#L84), [vlastní IBANy](../../src-tauri/src/services/categorization/own_accounts.rs#L22), [automatická kategorizace](../../src/pages/BankAccountDetail.tsx#L390). Ověřeno sondou.

### A02 · P1 · Import zahazuje identifikátor banky a zkresluje identitu protistrany

Viz odpověď výše. Chybějící kód banky komplikuje nejen zobrazení, ale i učení: stejné domácí číslo v různých bankách může být zpracováno jako jedna identita. Samotné označení pole „IBAN“ neodpovídá tomu, co presety do něj ukládají.

**Změna:** zavést strukturovaný identifikátor účtu, zachovat původní text a spojovat domácí číslo s bankovním kódem a zemí.

### A03 · P1 · Deduplikace může zahodit dvě různé skutečné platby

Náhradní klíč obsahuje datum, value date, částku jako text, směr a popis. **Neobsahuje měnu ani protistranu.** Na účtu s více měnami dvě platby `−100 EUR` a `−100 USD` se stejným datem a popisem způsobily import jediného řádku. Stejně dopadly dvě platby ve stejné měně různým příjemcům.

Ani doplnění polí neudělá z podobnosti jistotu: dvě totožné kávy jsou pořád dvě legitimní platby. Naopak porovnávání TEXT částek komplikuje shodu ručně/MCP zapsaného `100.00` s CSV `100`.

**Změna:** bankovní ID oddělit jako jistou duplicitu; podobnost jako „Možná duplicita“. Zahrnout měnu a identitu protistrany, kanonizovat peněžní hodnoty a respektovat násobnost shod v překrývajících se výpisech. Zajistit přístup ke všem sporným řádkům.

Důkaz: [klíč v souboru](../../src-tauri/src/services/csv_import/mod.rs#L63), [kontrola DB](../../src-tauri/src/services/dedup.rs#L43). Oba rozdílné scénáře ověřeny sondou.

### A04 · P1 · Vratky dávají různé výsledky v rozpočtech a cashflow

Rozpočty sčítají jen kategorie s kladným čistým výdajem. Cashflow odečte vratku napříč kategoriemi, ale záporný celkový výdaj a záporné měsíce ořízne na nulu. UI cashflow pak záporné skupiny bez kladného předchozího období skryje.

Reprodukce: restaurace `−1 000 Kč`, vratka potravin `+500 Kč`, bez nákupu potravin v období. Rozpočtový report hlásí výdaj `1 000 Kč`, cashflow `500 Kč`; vratka nemusí být v rozpisu vidět. Při vratkách větších než výdaje se ztratí část skutečného čistého přírůstku. Měsíční ořez a ořez celého období se navíc mohou lišit.

**Změna:** zachovat znaménko i v agregacích. Odděleně evidovat hrubé nákupy, vratky a čisté výdaje. Spotřební limit lze hodnotit podle nezáporné spotřeby, ale tím se nesmí přepisovat finanční součet. U vratky přes hranici období ukázat datum přijetí a původní nákup, je-li propojený.

Důkaz: [rozpočtové ořezávání](../../src-tauri/src/services/budgeting.rs#L300), [cashflow ořezávání](../../src-tauri/src/services/cashflow_actuals.rs#L280), [skrytí skupin](../../src/pages/Cashflow.tsx#L97). Rozdíl 1 000/500 ověřen sondou.

### A05 · P1 · Příjem existuje jen v jedné pevně zadané kategorii; jeho storno mizí

Příchozí položka se počítá jako příjem pouze pro `cat_income` nebo pokud nemá kategorii. Vlastní kategorie „Mzda“/„Pronájem“ je vyhodnocena jako vratka výdaje. Odchozí transakce v `cat_income` se úplně ignoruje, takže vrácená část mzdy nesníží příjem.

Sonda: mzda `+1 000`, její vrácení `−100`, vlastní kategorie příjmu `+2 000`. Cashflow ukáže příjem pouze `1 000`, další `2 000` vede jako záporný výdaj.

**Změna:** kategorie musí mít explicitní roli nebo povaha transakce samostatný atribut. Refund a příjem se nesmějí odvozovat pouze ze jména/ID kategorie. Překategorizování musí mít předvídatelný dopad na report.

Důkaz: [interpretace cashflow](../../src-tauri/src/services/cashflow_actuals.rs#L225), [interpretace rozpočtu](../../src-tauri/src/services/budgeting.rs#L234). Ověřeno sondou.

### A06 · P1 · Detail účtu sčítá různé měny jako jednu

Import i ruční formulář dovolují měnu transakce odlišnou od měny účtu. `summarizeFlow` a `reconstructBalance` měnu nečtou. Detail a seznam pak jejich součty konvertují jako měnu celého účtu. Například odchozích `100 EUR` a `100 CZK` se může stát výdaj `200 CZK` na korunovém účtu. Rekonstrukce zůstatku používá stejnou chybnou jednotku.

**Změna:** buď mít každý měnový podúčet odděleně a přijímat jen jeho zaúčtovanou měnu, nebo ukládat původní i skutečně zaúčtovanou částku. Nepočítat smíšený účet prostým součtem nominálních čísel. Směna je samostatná operace, včetně poplatku.

Důkaz: [pomocné výpočty](../../src/utils/bank-activity.ts#L35), [jejich použití](../../src/pages/BankAccountDetail.tsx#L380), [měna ruční transakce](../../src/components/bank-accounts/AddTransactionModal.tsx#L106). Ověřeno kódem.

### A07 · P1 · Změna transakcí neobnovuje všechny odvozené pohledy

Ruční přidání obnovuje jen transakce účtu. Ruční kategorizace a automatické zařazení v detailu také neinvalidují rozpočtové reporty ani cashflow. Přijetí AI návrhu naopak oba reporty obnovuje. Mazání účtu neobnovuje rozpočtový report a seznam transakcí kategorií má vlastní klíč, který jednotlivé mutace obecně neobnovují.

Cashflow má vynucené obnovení při vstupu, což část problému zmírňuje; nevylučuje ale neaktuální otevřený pohled nebo jiný report. UI tak závisí na tom, jak přesně uživatel provedl stejnou logickou změnu.

**Změna:** jeden společný kontrakt mutací peněžních dat. Obnovit reporty, drill-downy, počty nezařazených a relevantní účty. Snapshoty řešit podle existujících pravidel a toho, zda operace mění evidovaný zůstatek.

Důkaz: [mutace transakcí](../../src/hooks/use-bank-account-mutations.ts#L172), [změna kategorie](../../src/pages/BankAccountDetail.tsx#L464), [automatické zařazení](../../src/pages/BankAccountDetail.tsx#L407), [drill-down klíč](../../src/components/budgeting/CategoryTransactionsDialog.tsx#L64).

### A08 · P1 · Starý nebo cizoměnový výpis může přepsat dnešní zůstatek

Výsledek importu vrací `lastBalance` bez vlastní měny a data, ke kterému platí. Následná akce jej zapisuje jako zůstatek účtu. Nabídka kontroluje pouze rozdíl čísel. Výpis z minulého roku tak může nahradit dnešní zůstatek. U výpisu v jiné měně číslo přejde do měny účtu bez konverze nebo kontroly.

Sonda potvrdila, že výpis s `100 EUR` pro účet CZK vrací pouze `"100"`. Neprováděl jsem přepis reálného účtu. Rekonstrukce grafu dále předpokládá, že ručně zadaný zůstatek je dnešní a historie transakcí úplná.

**Změna:** zůstatek evidovat jako `částka + měna + platnost ke dni + zdroj`. Nabízet „Potvrdit zůstatek k 30. 9.“. Starší výpis nesmí automaticky působit jako dnešní stav. Neúplný graf označit jako odhad.

Důkaz: [výběr zůstatku](../../src-tauri/src/services/csv_import/mod.rs#L361), [podmínka nabídky](../../src/components/bank-accounts/csv-import/import-config.ts#L413), [aktualizace](../../src/components/bank-accounts/csv-import/NextSteps.tsx#L116). Ověřeno sondou a kódem.

### A09 · P1 · Nečíselné konečné hodnoty projdou validací backendu

`InsertBankTransaction.validate()` přijímá `NaN` a kladné `inf`: kontrola `amount <= 0` na to nestačí. CSV parser má vlastní ochrany, takže nejde o tvrzení, že běžné CSV automaticky propustí tyto hodnoty. Jde o chybu sdílené hranice zápisu dostupné i jiným vstupům. Rozpočtový model používá podobnou kontrolu.

**Změna:** explicitně vyžadovat konečnou kladnou hodnotu, platnou měnu a doménovou přesnost; pro peníze zvážit decimal reprezentaci ve výpočtech. TEXT v DB sám o sobě nezaručuje přesnost ani validitu.

Důkaz: [validace transakce](../../src-tauri/src/models/bank_transactions.rs#L118), [validace rozpočtu](../../src-tauri/src/models/budgeting.rs#L31). Transakční validace ověřena sondou.

### A10 · P1 · Neznámý kurz dává nesprávné součty; historický fallback není v reportu rozlišen

Chybějící historické kurzy přecházejí na současný kurz. Úplně neznámý kurz používá poměr `1 měnová jednotka = 1 CZK`. Existuje log a upozornění v globálním stavu dat — problém tedy není zcela skrytý. Samotné částky reportu přesto vypadají použitelné a výsledek nenese kvalitu použitého kurzu.

Globální kategorizační sada nedokládá, že pro danou zemi fungují kurzy, bankovní identifikátory a exporty. Převod do zobrazované měny navíc probíhá přes CZK aktuálním kurzem, což není totéž jako historický součet v uživatelově domácí měně.

**Změna:** chybějící kurz neoceňovat 1:1; zobrazit neoceněnou část zvlášť. U odhadovaného historického kurzu přenést stav až do reportu a exportu. Rozlišit interní účetní bázi CZK od uživatelovy reportovací měny.

Důkaz: [kurzový fallback](../../src-tauri/src/services/currency.rs#L45), [historické kurzy](../../src-tauri/src/services/currency.rs#L477), [existující upozornění](../../src/components/shell/DataStatus.tsx#L66).

### A19 · P1 · Čekající transakce vstupují do „skutečných“ reportů

Transakční model podporuje `pending`, ale SQL rozpočtů a skutečného cashflow nefiltruje stav. Sonda s jednou zaúčtovanou a jednou čekající platbou po 100 Kč vrací v obou reportech výdaj 200 Kč. Revolut preset filtruje COMPLETED již při importu, což tento konkrétní vstup chrání; není to ochrana společného reportu pro jiné cesty zápisu.

**Změna:** do skutečných součtů zahrnout zaúčtované položky. Čekající pohyby ukazovat odděleně jako očekávané a při následném zaúčtování je propojit nebo nahradit, aby nevznikla druhá platba. Zrušená autorizace nesmí trvale zůstat výdajem.

Důkaz: [SQL skutečného cashflow](../../src-tauri/src/services/cashflow_actuals.rs#L171), [SQL rozpočtů](../../src-tauri/src/services/budgeting.rs#L166), sonda `audit_pending_transactions_are_included_in_actuals_and_budgets`.

## Další funkční a UX problémy

### A11 · P2 · Dlouhá historie je bez zjevného upozornění oříznutá

Detail načte maximálně 1 000 transakcí; filtrování, hledání a kategorizace potom běží lokálně nad tímto výřezem. Graf a měsíční statistiky mají limit 5 000, přehled posledních 30 dní 1 000. API sice vrací celkový počet, ale UI neumožňuje načíst další stránky. Hledání starší platby může vrátit „nic“, přestože v DB existuje.

Náhled importu zpřístupňuje jen prvních 200 řádků, zatímco počty platí pro celý soubor. „Zobrazit vše“ odhalí pouze vrácený výřez; jednotlivou možnou duplicitu za řádkem 200 uživatel nemůže v náhledu vynutit.

**Změna:** stránkované hledání a filtry na backendu, agregace bez limitu; stránkování celého importního náhledu a samostatný seznam všech chyb/duplicit.

Důkaz: [načítání detailu](../../src/pages/BankAccountDetail.tsx#L202), [aktivity účtů](../../src/hooks/use-bank-account-activity.ts#L34), [limit náhledu](../../src-tauri/src/services/csv_import/mod.rs#L43).

### A12 · P2 · Selhání reportu vypadá jako nekonečné načítání

Cashflow a rozpočty neřeší query error jako vlastní stav. Podmínka `!data` / `!report` ponechá skeleton i po chybě. Uživatel nedostane příčinu ani cestu k obnově.

**Změna:** rozlišit načítání, chybu, chybějící výpisy, nulový tok a dostupná data; nabídnout „Zkusit znovu“. Nezaměňovat chybějící podklady za finanční nulu.

Důkaz: [cashflow](../../src/pages/Cashflow.tsx#L271), [rozpočty](../../src/pages/Budgeting.tsx#L375).

### A13 · P2 · Editace limitu mění i historické vyhodnocení rozpočtů

Cíl je jeden záznam kategorie + timeframe bez účinnosti od určitého období. Každý historický report používá současný měsíční limit, pro čtvrtletí ×3 a rok ×12. Změna limitu v říjnu proto změní i vyhodnocení srpna.

**Změna:** výchozí změna „Od tohoto měsíce“, verzované limity podle období; samostatná explicitní možnost přepočíst minulost. Srozumitelně rozlišit opakovaný limit a jednorázový rozpočet.

Důkaz: [upsert cíle](../../src-tauri/src/services/budgeting.rs#L73), [použití v reportu](../../src-tauri/src/services/budgeting.rs#L144).

### A14 · P2 · Doporučený limit a průměr cashflow ignorují pokrytí dat

Doporučený měsíční výdaj dělí součet vždy šesti předchozími měsíci, i když uživatel importoval jediný měsíc. Cashflow dělí celkový příjem/výdaj všemi měsíci zvoleného intervalu, včetně aktuálního neukončeného měsíce a měsíců bez podkladů.

V běžícím pohledu byl aktuální měsíc prázdný, ale zahrnutý do dvanáctiměsíčního průměru. Samo o sobě to není aritmetická chyba; pro nastavení rozpočtu je to zavádějící odhad bez kontextu.

**Změna:** evidovat importované intervaly po účtech; ukazovat „Průměr ze 3 doložených celých měsíců“. Bez pokrytí nabídnout orientační hodnotu s jasným označením nebo doporučení vynechat.

Důkaz: [šestiměsíční doporučení](../../src/components/budgeting/BudgetGoalDialog.tsx#L106), [průměry cashflow](../../src/pages/Cashflow.tsx#L302).

### A15 · P2 · Srovnání období není souměřitelné a YTD není jasně pojmenované

6/12 měsíců obsahuje aktuální částečný měsíc; předchozí interval obsahuje celé měsíce. YTD se srovnává s stejně dlouhým intervalem bezprostředně před 1. lednem, nikoli automaticky s loňským YTD. „Rok 2026“ a „vs. minulé“ neříkají přesně, co se s čím porovnává. Chybějící minulá data se mohou tvářit jako nový zdroj příjmů.

**Změna:** zobrazit přesná data porovnání a pokrytí. Nabídnout dokončené měsíce nebo stejný počet dní. Pro YTD použít jasně definované loňské YTD, případně současnou variantu výslovně pojmenovat.

Důkaz: [hranice období](../../src-tauri/src/services/cashflow_actuals.rs#L64), [etikety a srovnání](../../src/pages/Cashflow.tsx#L74).

### A16 · P2 · Učení kategorie je příliš široké a není navázané na úspěch uložení

Výběr kategorie standardně zapíná „Zapamatovat pro příště“. Je-li přítomen identifikátor účtu, vzniká pravidlo pro celý tento účet, i když platby znamenají různá témata. Směr nebo jiné podmínky se neučí. Jedna organizace může například posílat mzdu i refundaci a zároveň inkasovat nákup.

Callback uložení není awaitovaný; učení může proběhnout, i když změna transakce selže. Samotná služba učí paměť před uložením do DB. Chyba DB může zanechat dočasně odlišné chování před a po restartu. Při hromadném zařazování detail nejprve ukáže lokální kategorie, pak zapisuje více položek a při částečném selhání může oznámit úspěch podle počtu rozpoznaných shod.

**Změna:** oddělit „Jen tuto transakci“ a „Vytvořit pravidlo…“, ukázat rozsah pravidla a ukázkové shody. Učení a změnu zapsat bezpečně, teprve potom měnit paměť/UI. Hromadná akce má vrátit skutečně uložené a neúspěšné položky.

Důkaz: [výběr a učení](../../src/components/bank-accounts/CategorySelector.tsx#L116), [výchozí zapamatování](../../src/components/bank-accounts/CategorySelector.tsx#L341), [paměť před DB](../../src-tauri/src/services/categorization/learn.rs#L131), [hromadné zařazení](../../src/pages/BankAccountDetail.tsx#L394).

### A17 · P2 · Plánované cashflow může dál počítat splacený úvěr a stejné položky dvakrát

Obě úvěrové části plánu načítají pouze uloženou měsíční splátku bez kontroly současného zůstatku a ukončení úvěru. Pokud splacenému úvěru zůstane kladná `monthly_payment`, plán jej dál započítá. Ruční položky ve stejné kategorii se připojují k automatickým jako další položky; neexistuje vazba „nahrazuje tuto automatickou položku“.

Kladným bodem je již existující odlišení automatického a ručního zdroje v UI. Přesto není jasné, zda plán reprezentuje reálné splatnosti nebo roční částky rozdělené na dvanáct měsíců. Název „Zbývá k investování“ může vzbudit dojem volné hotovosti, ačkoli jde o rozdíl odhadů bez kalendáře plateb a rezerv.

**Změna:** respektovat aktivní stav a aktuální dluh, umožnit nahradit automatickou položku, rozlišit normalizovaný měsíční odhad a časový plán plateb. Přejmenovat výsledek na „Plánovaný měsíční přebytek“.

Důkaz: [úvěry v plánu](../../src-tauri/src/commands/cashflow.rs#L256), [druhá úvěrová část](../../src-tauri/src/commands/cashflow.rs#L585), [normalizace frekvence](../../src-tauri/src/commands/cashflow.rs#L70). Ověřeno kódem; reálný splacený úvěr nebyl měněn.

### A18 · P2 · Datum valuty se sice rozpoznává, ale neukládá

Detektor zná `valueDate`; importní konfigurace pro něj nemá samostatné pole a parser zapisuje vždy `value_date: None`. Uživatel tak nemá přístup k dvěma odlišným datům uvedeným bankou. Pro běžný report je vhodné jedno stabilně zvolené datum zaúčtování; metadata ale nemají beze stopy mizet.

**Změna:** volitelně zachovat datum valuty v detailu, vysvětlit, které datum řídí report. Ambiguity DD/MM vs. MM/DD v mezinárodních exportech explicitně potvrdit pomocí příkladů.

Důkaz: [zápis řádku](../../src-tauri/src/services/csv_import/rows.rs#L438), [konfigurace](../../src-tauri/src/services/csv_import/types.rs#L48).

## Navrhovaná změna produktu

### Jedno pravidlo pro finanční význam

Povahu toku oddělit od kategorie. Minimální model:
- směr a zaúčtovaná částka v měně účtu;
- povaha: příjem, výdaj, vratka, vlastní převod, přesun do investice, změna dluhu, úprava;
- uživatelská kategorie a původ jejího přiřazení;
- případná vazba na původní nákup nebo druhou stranu převodu;
- původ dat: import, soubor/řádek, manuální vstup, externí nástroj.

Názvy a přesný rozsah jsou návrh, nikoli hotový schválený wire contract. Dnešní `credit/debit` může zůstat jako směr. Kategorie musí přestat být skrytým účetním přepínačem.

| Operace | Detail účtu | Souhrn domácnosti | Spotřební rozpočet |
|---|---|---|---|
| Mzda | Příchozí pohyb | Příjem | Nečerpá limit |
| Nákup potravin | Odchozí pohyb | Výdaj | Čerpá potraviny |
| Vratka nákupu | Příchozí pohyb | Vratka, zachované znaménko | Snižuje odpovídající spotřebu |
| Převod na vlastní spořicí účet | Pohyb na obou účtech | Samostatný převod; ne příjem/výdaj domácnosti | Nečerpá limit |
| Úhrada vlastní kreditní karty, jejíž nákupy jsou importované | Převod/snížení dluhu | Nezapočítat znovu nákupy | Nečerpá podruhé |
| Vklad k brokerovi | Odchozí pohyb | Přesun do investic odděleně od spotřeby | Výchozí spotřební limit nečerpá |
| Výběr z brokera | Příchozí pohyb | Přesun z investic; automaticky není zisk | Nezvyšuje běžný příjem |
| Směna měn mezi vlastními zůstatky | Dvě měnové strany | Převod; poplatek samostatný výdaj | Jen relevantní poplatek |
| Čerpání půjčky | Příchozí pohyb | Financování, nikoli výdělek | Nezvyšuje disponibilní příjem |
| Splátka půjčky | Odchozí pohyb | Odtok hotovosti; jistina/úrok dle dostupných dat | Úrok/náklad a plán splátek explicitně |

U cashflow musí být vidět, zda uživatel sleduje všechny pohyby hotovosti, nebo příjmy a spotřebu. Přesuny do investic nelze univerzálně „skrýt“: snižují bankovní hotovost, ale nejsou spotřebou ani poklesem celkového majetku.

`excludeFromBalance` je dnes výslovně vyloučení z portfolia. Není to filtr reportů. Proto nepovažuji zahrnutí těchto transakcí do cashflow za automatickou chybu. UX však potřebuje oddělenou volbu účtů pro report: osobní, společné, podnikatelské či investiční.

### Import jako krátká, kontrolovatelná operace

1. **Vybrat účet a soubor.** Rozpoznaná banka a měna. Shoda s identitou účtu, pokud ji export obsahuje.
2. **Zkontrolovat výsledek.** Datum od–do, příchozí/odchozí součty po měnách, nové položky, jisté duplicity, možné duplicity, chyby a vlastní převody.
3. **Importovat N transakcí.** Výsledek s počtem skutečně přidaných položek a přístupem k chybám.

Mapování, encoding, oddělovač a technická pole schovat do **„Upravit rozpoznání“**, automaticky otevřeného při nejistotě. Na známém formátu nemá uživatel vyplňovat prázdný konfigurační formulář. Neznámý formát má zachovat současnou možnost ručního nastavení.

Zůstatek potvrzovat samostatně s měnou a datem. Import nebude bez vysvětlení tvrdit, že změnil zůstatek. Uchovat importní profil pro příští výpis ze stejného zdroje.

### Transakce jako hlavní pracovní místo

Doporučuji společný pohled **Transakce** přes vybrané účty, s frontou „Ke kontrole“. V detailu účtu může zůstat stejný seznam s filtrem na účet.

Řádek má ukázat příjemce, částku s měnou, datum, kategorii a případně stav „Vlastní převod“ nebo „Čeká na zaúčtování“. Detail otevře původ, celé číslo účtu, bankovní ID, obě data, vysvětlení kategorizace a dopad na report.

Nezařazené transakce nemají uživatele nejprve posílat do technické integrace MCP. V běžícím UI velký banner mezi statistikami a seznamem vysvětluje externí AI asistenta a vestavěný server. To odvádí pozornost od základního úkolu. MCP ponechat v Integracích; „Ke kontrole“ musí fungovat bez něj.

### Rozpočty a cashflow

- **Rozpočty:** jasně oddělit „Zbývá v nastavených limitech“ a „Výdaje bez limitu“. Součet zbytků kategorií není disponibilní hotovost domácnosti.
- **Rozpočty:** označit od kterého měsíce limit platí; existující limity ukazovat i v měsíci bez transakcí.
- **Rozpočty:** při velkém nezařazeném objemu nabídnout zařazení, ne optimistické hodnocení limitů.
- **Cashflow:** dominantní číslo pro čistý tok a stav pokrytí; aktuální neúplný měsíc označit.
- **Cashflow:** každý součet otevře seznam započtených transakcí. Dnešní tabulka cashflow takový drill-down nemá, zatímco rozpočty jej mají.
- **Plán:** výslovně „Odhad“ a datum aktualizace. U ročních položek ukázat, že měsíční hodnota je rozpočítaná, nikoli skutečná splatnost.
- V detailu účtu použít **„Příchozí / Odchozí pohyby“**, pokud statistika zahrnuje i převody. Tím se odliší od příjmů/výdajů domácnosti.

### Co znamená požadovaná kvalita „jako od Apple“

Zachovat klidné plochy, typografii a konzistentní ovládací prvky. Zlepšit význam akcí, jasnost výsledku, prevenci chyb a možnost nápravy. Apple HIG doporučují usnadnit zadávání dat bez chyb a poskytnout zpětnou vazbu při ověřování; undo má umožnit bezpečné vracení akcí. [Entering data](https://developer.apple.com/design/human-interface-guidelines/entering-data), [Undo and redo](https://developer.apple.com/design/human-interface-guidelines/undo-and-redo).

Konkrétně:
- tlačítko pojmenuje výsledek: „Importovat 128 transakcí“, „Odstranit 128 transakcí“;
- žádné číselné závěry bez informace o podkladech;
- důležitá nejistota u příslušného čísla, ne jen v globálním menu;
- méně kroků u známé banky, opravitelné rozpoznání u neznámé;
- technické podrobnosti dostupné v detailu;
- stejné chování bez ohledu na to, zda změna přišla ručně, z CSV nebo přes nástroj.

## Globální použití: balíčky pravidel nejsou celé řešení

Pozitivní je současná podpora mnoha regionálních balíčků a normalizace zachovávající i nelatinková písma. CSV detekce obsahuje vícejazyčné názvy a backend drží konzistentní UTC kalendářní dny. Tyto základy není důvod přepisovat.

Pro globální vydání oddělit:
1. **Jazyk UI:** dnes en/cs.
2. **Zemi a formát exportu:** bankovní preset, pořadí data, separátory.
3. **Měnu účtu a měnu zaúčtování.**
4. **Reportovací měnu:** preference uživatele; ne zaměnit s CZK jako interní bází.
5. **Účetní identifikátory země:** IBAN nebo domácí číslo + bankovní identifikátor.
6. **Balíčky obchodníků:** více zemí současně podle použití.

IBAN země protistrany není jistý důkaz země, kde uživatel utrácí. Návrh balíčku proto zůstat návrhem. Balíčky mohou rozpoznat obchodníka, ale nemají samy rozhodovat, zda je příchozí platba příjem, refund nebo výběr z vlastního investičního účtu.

Pozor i na precision: aplikace podporuje měny s nulou i třemi desetinnými místy, dialog editace rozpočtu však konvertovanou hodnotu zaokrouhluje na dvě. Při pouhém otevření a uložení se proto může změnit limit ve třídesetinné měně. Zachovat původní hodnotu, pokud ji uživatel nezměnil.

## Scénáře pro release ověření

| Scénář | Požadovaný výsledek |
|---|---|
| Převod běžný → spořicí ve stejné měně | Mění oba účty, nezvyšuje příjem ani spotřebu domácnosti. |
| Jen jedna strana známého vlastního převodu | Nečeká na import protistrany, má dohledatelný vlastní cíl. |
| Převod přes hranici měsíce | Nezkreslí příjem/spotřebu ani jednoho měsíce; nesoulad páru je vidět. |
| Domácí číslo a ekvivalentní IBAN | Rozpoznají stejný účet, včetně prefixu a banky. |
| Stejné číslo v jiné bance | Zůstane jinou identitou. |
| Vlastní účet přidaný po importu | Nabídka kontroly starších převodů bez přepsání ručních voleb. |
| Směna EUR → CZK s poplatkem | Propojené strany v obou měnách; pouze poplatek jako výdaj. |
| Kreditní karta + bankovní úhrada karty | Nákupy se nezapočítají dvakrát; dluh má správné znaménko. |
| Výběr a následné utrácení hotovosti | Při evidenci peněženky nedochází k dvojímu výdaji; bez ní je politika výslovná. |
| Vklad/výběr brokera | Rozlišen od zisku/dividendy a od spotřeby. |
| Půjčka, její čerpání a splacení | Žádný falešný výdělek; plán přestane počítat splacenou splátku. |
| Vratka v následujícím období | Součet souhlasí mezi reporty a rozpisem, vratka nezmizí. |
| Refund mzdy a vlastní příjmová kategorie | Správný čistý příjem. |
| Dvě skutečné totožné platby | Uživatel může zachovat obě; nejsou bez vysvětlení zahozena data. |
| EUR/USD se stejným nominálem | Nejsou vzájemné duplicity. |
| Stejný export podruhé a překrývající se výpis | Bez navýšení počtu plateb; zohlednění násobnosti. |
| PENDING → BOOKED a zrušená autorizace | Čekající stav oddělen; žádný dvojí skutečný výdaj. |
| Samostatný Fee ve výpisu | Ověřit vůči konkrétnímu formátu, zda je již v Amount; případně importovat samostatně. |
| Starý výpis se zůstatkem | Nepřepíše dnešní stav bez data a vysvětlení. |
| Více měn v jednom exportu | Žádný součet různých jednotek; bezpečné podúčty nebo zaúčtované částky. |
| Jen jeden importovaný měsíc | Doporučení/průměry uvádějí omezené pokrytí. |
| Více než 1 000 transakcí a 200 řádků náhledu | Celá historie je dostupná, všechny sporné řádky lze řešit. |
| Chyba DB / částečné zařazení | Pravdivý výsledek; nenaučí se neuložené rozhodnutí. |
| Smazání importu po ruční změně jeho transakcí | Jasný aktuální rozsah mazání, dohledatelný původ a obnova. |
| Změna měny zobrazení a limit KWD/BHD/OMR | Žádná nechtěná změna hodnoty při editaci, zachování příslušné přesnosti. |
| Export s nejednoznačným 03/04/2026 | Viditelná volba pořadí data; oprava před zápisem. |
| Chybějící historický/aktuální kurz | Odhad je označen; neznámá částka se nesčítá jako 1:1. |
| Osobní/společné/podnikatelské účty | Jednoznačný rozsah reportu nezávislý na zahrnutí do jmění. |

Poplatky, cash wallet, kreditní karty a jednotlivé bankovní exporty v této tabulce jsou **akceptační scénáře**, nikoli automaticky potvrzené konkrétní chyby všech těchto formátů.

## Doporučené pořadí práce

1. **Společný finanční model a správnost:** A01–A10 a A19. Nejdřív rozhodnout jednotky, význam převodu/refundu/příjmu a pravidla pro dostupná data.
2. **Import a dohledatelnost:** přesná identita účtu, jisté/možné duplicity, úplný náhled, zůstatek k datu, zdroj každého řádku a bezpečné mazání.
3. **Rozpočty a reporty:** platnost limitů, pokrytí, symetrická období, dohledatelné součty a aktivní závazky v plánu.
4. **Zkrácení toku a copy:** výchozí automatické rozpoznání, názvy akcí, příchozí/odchozí vs. příjem/výdaj, přesunutí MCP mimo běžnou práci.
5. **Ověření na reprezentativních exportech:** CZ, země IBAN/SEPA, UK, US a více měn; nelatinková protistrana a měna se třemi desetinnými místy.

Vznik návrhu není pokyn k přepisování historických dat. Změny wire contractu a DB musí projít projektovou procedurou a append-only migrací; nejasné starší transakce patří do kontroly, nikoli do automatické domnělé opravy.

## Co funguje a audit neoznačil za chybu

- Současný CSV zápis je atomický: dávka a všechny zapisované řádky v jedné SQL transakci.
- Náhled a zápis používají stejný parser a stejnou kontrolu duplicit.
- Importované řádky mají vazbu na dávku; aktuální mazání umí tyto řádky odstranit.
- Řádky s chybami jsou hlášené, nulové a filtrované položky se rozlišují.
- Existují bankovní presety, kontrola jejich fixtures a uživatelské přepsání detekce.
- IBAN normalizace řeší mezery a velikost písmen; vlastní IBANy se obnovují při změně účtů.
- Vrácení částky do téže výdajové kategorie ve stejném období obecně snižuje výdaj.
- Rozpočty převádějí měnu uloženého limitu, neporovnávají nominálně EUR s CZK.
- Při chybějícím kurzu už existuje varování; výhrada se týká nesprávného náhradního součtu.
- AI návrh se učí po potvrzení uživatelem.
- Mazání kategorií má postup pro přeřazení závislostí; není slepým odstraněním všech odkazů.
- Rozlišení skutečnosti a plánu v navigaci a popiscích už existuje.

## Provedené ověření a artefakty

- `npm test`: **71 souborů, 977 testů prošlo**. Runner po dokončení hlásil opožděné ukončení procesu; exit code byl 0.
- `npm run typecheck`: **prošlo**, exit code 0.
- `cargo test`: **1 393 knihovních testů prošlo, 4 ignorované**, dále 7 + 10 testů binárních nástrojů a 1 doctest prošly. První sandboxový běh selhal pouze u dvou HTTP testů kvůli zákazu otevřít lokální port; opakování s oprávněním prošlo.
- **8 auditních sond prošlo.** Jsou charakterizační: potvrzují současné chování, včetně popsaných chyb; jejich zelený výsledek není doklad správnosti produktu.
- [Zdroj sond](../../docs/audits/2026-10-06-money-flows-probes.rs).
- [Výstup sond](../../docs/audits/2026-10-06-money-flows-probes-output.txt).

Pro reprodukci sond dočasně přidat na konec `src-tauri/src/services/mod.rs`:

```rust
#[cfg(test)]
#[path = "../../../docs/audits/2026-10-06-money-flows-probes.rs"]
mod audit_probes;
```

Pak spustit z `src-tauri` `cargo test audit_probes -- --nocapture` a tento dočasný blok odstranit. Sondy nepoužívají skutečnou databázi ani SQLCipher setup. V tomto auditu byl blok po ověření odstraněn; aplikační kód zůstal beze změny.

Audit neprovedl lint, produkční build, plný test klávesnice/VoiceOver ani fault injection celého desktopového importu. Passing existující sady proto není potvrzení produkční připravenosti uvedených scénářů.
