# Translation glossary

Shared terms and conventions for every Moony locale. English (`src/i18n/locales/en`) is the
source; Czech (`cs`) is the owner's own copy and the best guide to tone and intent. When a
term is missing here, pick one rendering and use it consistently across all namespaces of
that language.

## Never translate

Moony, AES-256, SQLCipher, MCP, Claude, CoinGecko, Yahoo Finance, ISIN, ticker symbols,
currency codes (CZK, EUR, …), file formats (CSV, XLSX, PDF, JSON), file names
(`moony-recovery-key.txt`), URLs, broker and bank names (XTB, Trading 212, Interactive
Brokers / IBKR, Revolut, Degiro, Fio, Air Bank, …), keyboard keys.

## Conventions

| | de | es | it | nb | pl | sv | ja | zh |
|---|---|---|---|---|---|---|---|---|
| Register | Sie | tú | tu | du | ty (lower-case) | du | です・ます | 您 |
| Quotes | „…“ | «…» | «…» | «…» | „…” | ”…” | 「…」 | “…” |
| ECB | EZB | BCE | BCE | ESB | EBC | ECB | ECB | 欧洲央行 |
| p.a. | p. a. | anual | annuo | p.a. | rocznie | per år | 年率 | 年化 |

- Sentence case for labels, titles and buttons (German capitalises nouns as usual).
- No exclamation marks. Keep `…` (single character) where English has it.
- Keep labels short: buttons, menu items, table headers and tabs must stay about as long as
  the English. Prefer the shorter of two correct renderings.
- `{{variables}}` keep their exact names and stay in the string; move them where the grammar
  needs. `{{month, lower}}` lower-cases a month label: es, it, nb, pl and sv write
  `{{month, lower}}` wherever the month is not the first word of the string; de, ja and zh
  never use `, lower`.
- Plurals: give exactly the forms `Intl.PluralRules` lists for the language —
  de/nb/sv `_one` `_other`; es/it `_one` `_many` `_other` (`_many` = same text as
  `_other`); pl `_one` `_few` `_many` `_other`; ja/zh only `_other`. Keep a `_zero` key
  wherever English has one. A key that is not a plural in English (e.g. `cat_other`, the
  "Other" category) stays a plain key.
- Numbers, dates and money are formatted by the app — never write them out in a string.
- `<b>…</b>` and `<link>…</link>` in a string are React components (`<Trans>`): keep each
  tag pair, translate the text inside, move the pair where the grammar needs it.

## Core terms

| English | de | es | it | nb | pl | sv | ja | zh |
|---|---|---|---|---|---|---|---|---|
| Overview | Übersicht | Resumen | Panoramica | Oversikt | Przegląd | Översikt | 概要 | 概览 |
| Net worth | Nettovermögen | Patrimonio neto | Patrimonio netto | Nettoformue | Majątek netto | Nettoförmögenhet | 純資産 | 净资产 |
| Portfolio | Portfolio | Cartera | Portafoglio | Portefølje | Portfel | Portfölj | ポートフォリオ | 投资组合 |
| Bank account | Bankkonto | Cuenta bancaria | Conto bancario | Bankkonto | Konto bankowe | Bankkonto | 銀行口座 | 银行账户 |
| Savings account | Sparkonto | Cuenta de ahorro | Conto deposito | Sparekonto | Konto oszczędnościowe | Sparkonto | 貯蓄口座 | 储蓄账户 |
| Transaction / payment | Transaktion / Zahlung | Movimiento / pago | Movimento / pagamento | Transaksjon / betaling | Transakcja / płatność | Transaktion / betalning | 取引 / 支払い | 交易 / 付款 |
| Payee (who is paid) | Empfänger | Beneficiario | Beneficiario | Mottaker | Odbiorca | Mottagare | 支払先 | 收款方 |
| Counterparty (the other side, in or out) | Gegenpartei | Contraparte | Controparte | Motpart | Kontrahent | Motpart | 取引相手 | 交易对方 |
| Category | Kategorie | Categoría | Categoria | Kategori | Kategoria | Kategori | カテゴリ | 类别 |
| Categorization rules | Kategorisierungsregeln | Reglas de categorización | Regole di categorizzazione | Kategoriseringsregler | Reguły kategoryzacji | Kategoriseringsregler | 分類ルール | 分类规则 |
| Budget | Budget | Presupuesto | Budget | Budsjett | Budżet | Budget | 予算 | 预算 |
| Cashflow | Cashflow | Flujo de caja | Flusso di cassa | Kontantstrøm | Przepływy pieniężne | Kassaflöde | キャッシュフロー | 现金流 |
| Income / expenses | Einnahmen / Ausgaben | Ingresos / gastos | Entrate / uscite | Inntekter / utgifter | Przychody / wydatki | Inkomster / utgifter | 収入 / 支出 | 收入 / 支出 |
| Salary | Gehalt | Salario | Stipendio | Lønn | Wynagrodzenie | Lön | 給与 | 工资 |
| Refund | Erstattung | Reembolso | Rimborso | Refusjon | Zwrot | Återbetalning | 返金 | 退款 |
| Own-account transfer | Umbuchung | Traspaso | Giroconto | Overføring mellom egne kontoer | Przelew własny | Överföring mellan egna konton | 口座間振替 | 账户间转账 |
| Stocks | Aktien | Acciones | Azioni | Aksjer | Akcje | Aktier | 株式 | 股票 |
| Holding / position | Position | Posición | Posizione | Beholdning | Pozycja | Innehav | 保有銘柄 | 持仓 |
| Dividend | Dividende | Dividendo | Dividendo | Utbytte | Dywidenda | Utdelning | 配当 | 股息 |
| Bond | Anleihe | Bono | Obbligazione | Obligasjon | Obligacja | Obligation | 債券 | 债券 |
| Coupon | Kupon | Cupón | Cedola | Kupong | Kupon | Kupong | クーポン | 票息 |
| Maturity | Fälligkeit | Vencimiento | Scadenza | Forfall | Termin wykupu | Förfall | 満期 | 到期 |
| Face value | Nennwert | Valor nominal | Valore nominale | Pålydende | Wartość nominalna | Nominellt värde | 額面 | 面值 |
| Crypto | Krypto | Cripto | Cripto | Krypto | Kryptowaluty | Krypto | 暗号資産 | 加密货币 |
| Real estate | Immobilien | Inmuebles | Immobili | Eiendom | Nieruchomości | Fastigheter | 不動産 | 房产 |
| Other assets | Sonstiges Vermögen | Otros activos | Altri beni | Andre eiendeler | Inne aktywa | Övriga tillgångar | その他の資産 | 其他资产 |
| Loan / mortgage | Kredit / Hypothek | Préstamo / hipoteca | Prestito / mutuo | Lån / boliglån | Kredyt / kredyt hipoteczny | Lån / bolån | ローン / 住宅ローン | 贷款 / 房贷 |
| Interest rate | Zinssatz | Tipo de interés | Tasso di interesse | Rente | Oprocentowanie | Ränta | 金利 | 利率 |
| Insurance / premium | Versicherung / Beitrag | Seguro / prima | Assicurazione / premio | Forsikring / premie | Ubezpieczenie / składka | Försäkring / premie | 保険 / 保険料 | 保险 / 保费 |
| Main currency | Hauptwährung | Moneda principal | Valuta principale | Hovedvaluta | Waluta główna | Huvudvaluta | 基準通貨 | 主货币 |
| Exchange rate | Wechselkurs | Tipo de cambio | Tasso di cambio | Valutakurs | Kurs wymiany | Växelkurs | 為替レート | 汇率 |
| Projection | Prognose | Proyección | Proiezione | Prognose | Prognoza | Prognos | 予測 | 预测 |
| Milestones | Meilensteine | Hitos | Traguardi | Milepæler | Kamienie milowe | Milstolpar | マイルストーン | 里程碑 |
| Stock monitor / watchlist | Aktienmonitor / Beobachtungsliste | Monitor de acciones / lista de seguimiento | Monitor azioni / lista di osservazione | Aksjemonitor / følgeliste | Monitor akcji / lista obserwowanych | Aktiebevakning / bevakningslista | 株式モニター / ウォッチリスト | 股票监控 / 自选列表 |
| Lock / unlock | Sperren / Entsperren | Bloquear / desbloquear | Blocca / sblocca | Lås / lås opp | Zablokuj / odblokuj | Lås / lås upp | ロック / ロック解除 | 锁定 / 解锁 |
| Password | Passwort | Contraseña | Password | Passord | Hasło | Lösenord | パスワード | 密码 |
| Recovery key | Wiederherstellungsschlüssel | Clave de recuperación | Chiave di recupero | Gjenopprettingsnøkkel | Klucz odzyskiwania | Återställningsnyckel | 復旧キー | 恢复密钥 |
| Backup | Sicherung | Copia de seguridad | Backup | Sikkerhetskopi | Kopia zapasowa | Säkerhetskopia | バックアップ | 备份 |
| Import / export | Import / Export | Importar / exportar | Importa / esporta | Import / eksport | Import / eksport | Import / export | インポート / エクスポート | 导入 / 导出 |
| Settings | Einstellungen | Ajustes | Impostazioni | Innstillinger | Ustawienia | Inställningar | 設定 | 设置 |

## Phone tab labels

The phone app's tab bar (`mobile` namespace, `tabs.*`) gives each label about 85 px at 360 dp, so a tab
may carry a shorter form of a core term. Each such form is listed here, so desktop and phone do not drift:

| Term | Tab label on the phone | Everywhere else |
|---|---|---|
| Cashflow (pl) | Przepływy | Przepływy pieniężne |

Every other tab label is the core term above or the tab name from the mobile spec §6 (Finance, Wealth).
