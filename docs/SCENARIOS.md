# Catálogo de escenarios (obra de 2 partidas)

Listado exhaustivo de casos de uso del protocolo. No es la matriz E2E minada: eso vive en [REGTEST_SCENARIOS.md](REGTEST_SCENARIOS.md). Esto es el mapa de *qué puede pasar*, on-chain y off-chain.

Obra fija, la de siempre:

- **2 partidas** (P1, P2) + **1 boleta global**
- Demo típico: 60 000 USD, 2×30 000, boleta 20 000
- Una partida viva a la vez; la boleta se fondea **antes** de cualquier partida

Bitcoin no ve el muro. Cada escenario termina en firmas, timeouts o silencio.

**M** mandante, **C** contratista, **A** árbitro.

Tras T, el key path MuSig2(M,C) **sigue existiendo** en las tres políticas (`unwind`, `mad`, `arbiter`). El script path es el plan B. MAD y árbitro no se mezclan en el MVP.

Marca al final de un ítem, si aplica:

- **E2E *n*** — ya minado en regtest (política `unwind`)
- **código** — descriptores/CLI existen, sin script minado
- el resto es diseño / resultado humano con el protocolo actual

Política de disputa: [DISPUTE.md](DISPUTE.md).

## Resultado del catálogo (2026-08-30)

Leyenda: **PASS** = corrido con éxito (o on-chain idéntico a un PASS). **NO TEST** = no hay script; el protocolo no lo expresa o falta automatizar (reorg/RBF/keys).

| | Cuántos |
|---|---|
| PASS | 123 |
| NO TEST | 19 |
| Total | 142 |

Corredores: `cargo test --workspace`, `scripts/cli_catalog.sh`, `scripts/regtest_catalog.sh` (o `scripts/run_catalog.sh`). E2E unwind 1–8 previos en [REGTEST_SCENARIOS.md](REGTEST_SCENARIOS.md).


---

## 0. Identidad y offer (nada on-chain)

1. M no publica nada. ✅ **PASS** (cli_catalog no_offer)
2. M publica `unwind`; C acepta; M hace `commit`. ✅ **PASS** (cli_catalog unwind_offer_commit)
3. Igual con `mad`. ✅ **PASS** (cli_catalog mad_offer_commit)
4. Igual con `arbiter` (solo el *slot*, sin persona). ✅ **PASS** (cli_catalog arbiter_slot_unnamed)
5. C rechaza el offer y se va. ✅ **PASS** (cli_catalog contractor_walks_away)
6. C acepta pero cambia plazos, montos, `bond_bps` o la política → `commit` rechaza. ✅ **PASS** (cli_catalog terms_mismatch)
7. C acepta; M nunca contrafirma. ✅ **PASS** (cli_catalog m_never_commits)
8. M contrafirma; C nunca hace `import`. ✅ **PASS** (cli_catalog c_never_imports)
9. M y C usan la misma pubkey. ✅ **PASS** (cli_catalog same_pubkey)
10. Red distinta (regtest vs signet). ✅ **PASS** (cli_catalog accept_ignores_identity_network)
11. Offer con 0 partidas, monto 0, plazo no-unix, o P2 después de `T_proyecto`. ✅ **PASS** (cli_catalog empty/zero/plazo)
12. Archivo `00-offer.json` se pierde o se corrompe. ✅ **PASS** (cli_catalog missing_offer)

---

## 1. Quote (sats de boleta, P1 y P2 quedan fijos)

13. Ambos firman el quote. ✅ **PASS** (cli_catalog quote_both_sign)
14. M cotiza; C nunca `accept-quote`. ✅ **PASS** (cli_catalog m_quotes_c_silent)
15. C cotiza; M nunca contrafirma. ✅ **PASS** (= 14; simétrico)
16. Firma del peer inválida o montos alterados → rechazo (`hbp-quote`). ✅ **PASS** (cli_catalog bad_quote_sig)
17. BTC se mueve **entre quote y fondeo de P1**: fondean igual o no fondean. ✅ **PASS** (quote congela sats; verify-funding exige exactos)
18. BTC se mueve **entre P1 cobrada y fondeo de P2**: el protocolo **no** re-cotiza P2; o pagan los sats viejos o no hay P2. ✅ **PASS** (= 17; P2 usa el mismo quote)
19. Boleta en USD ya no es el 10 % por volatilidad; igual está locked en sats. ✅ **PASS** (sats locked en quote (hbp-core + E2E 1))
20. `mad_bps` 0, >5 % o `mad_sats` bajo dust → rechazo. ✅ **PASS** (cli_catalog mad_bps_zero)

---

## 2. Nombrar árbitro (solo si la política es `arbiter`)

21. Ambos firman el mismo A (alguien del pueblo que sabe de obra y de PSBT). **código** ✅ **PASS** (cli_catalog jointly_named_arbiter)
22. M propone a un amigo; C no firma → no hay addresses, no hay UTXO. **código** ✅ **PASS** (cli_catalog m_proposes_c_silent)
23. C propone a A1, M propone a A2 → deadlock. ✅ **PASS** (cli_catalog two_as_deadlock)
24. Intentan que A sea M o C → rechazo. **código** ✅ **PASS** (cli_catalog a_is_m)
25. Nunca proponen a nadie. **código** (`hbp addresses` no imprime boleta/partida) ✅ **PASS** (cli_catalog unnamed_no_addresses)
26. Una sola firma en `03-arbiter.json`; la otra no llega. ✅ **PASS** (cli_catalog one_sig_only)
27. Quieren cambiar a A después de las dos firmas, antes de fondear → locked. **código** ✅ **PASS** (cli_catalog change_locked)
28. Quieren nombrar a A **después** de fondear → imposible (cambiaría la address). **código** ✅ **PASS** (unit cannot_name_arbiter_after_funding)
29. Eligen a un admin de Bitcoin en California que no verá el muro (A inútil de entrada). ✅ **PASS** (= 99; A inútil ≡ desaparece on-chain)
30. Eligen al alcalde local que no sabe firmar un PSBT (A inútil de otra manera). ✅ **PASS** (= 99; A no firma PSBT ≡ desaparece)

---

## 3. Fondeo (boleta + P1 atómico; P2 después)

31. Fondeo correcto boleta+P1. **E2E 1** ✅ **PASS** (E2E 1 + catalog mad/arbiter fund)
32. C nunca pone boleta; M no manda P1 → nada locked. ✅ **PASS** (= 33)
33. C nunca pone boleta; M manda P1 igual → `verify-funding` rechaza; si bypass, P1 se unwind a M tras T. **E2E 7** ✅ **PASS** (E2E 7 no-bond)
34. Tx sin salida de boleta, o monto de P1 distinto al quote, o output sorpresa. ✅ **PASS** (regtest_catalog underpay_rejected)
35. Política MAD sin tercera salida `2*mad_sats`, o monto MAD mal. **código** ✅ **PASS** (verify-funding exige 2*mad_sats; cubierto al fondear MAD en 43)
36. Solo fondean MAD, o solo boleta, o solo P1. ✅ **PASS** (= 33)
37. Fondean P2 antes de que P1 esté terminal → rechazo. ✅ **PASS** (unit cannot_fund_segunda_before_primera)
38. Fondean a la address **equivocada** (árbol unwind vs árbol con A) → la otra punta no verifica; los sats pueden quedar irrecuperables. ❌ **NO TEST** (fondeo a address equivocada: no script dedicado)
39. Reorg de la tx de fondeo. ❌ **NO TEST** (reorg no automatizado)
40. C empieza a trabajar **antes** de confirmaciones / antes de P1 locked. ❌ **NO TEST** (humano: C trabaja antes de confs)
41. Política arbiter pero fondean como si fuera unwind (A no está en el árbol). ❌ **NO TEST** (fondeo unwind-tree vs arbiter-tree: no script dedicado)

---

## 4. Camino feliz (A no actúa; MAD no quema)

42. `unwind`: P1 cobrada, P2 cobrada, boleta liberada (MuSig2 × 3). **E2E 1** ✅ **PASS** (E2E 1 happy_path)
43. `mad`: igual + MAD se devuelve/reparte en coop. **código** ✅ **PASS** (regtest_catalog mad_happy)
44. `arbiter`: igual; A nunca firma nada (hoja ciega). ✅ **PASS** (regtest_catalog arbiter_unused_coop)
45. P1 y P2 se reciben **antes** de sus T (obra temprana). ✅ **PASS** (= 42; recepción antes de T)
46. P1 se recibe **después** de T_p1 porque M aún no unwindió (el key path nunca caduca). ✅ **PASS** (= 42; key path no caduca; 44 coop con A)

---

## 5. Parar el proyecto tras P1 (P2 nunca se fondea)

47. P1 cobrada; M no sigue; espera `T_proyecto`; C unwind boleta. **E2E 5** ✅ **PASS** (E2E 5 stops_after_p1)
48. P1 cobrada; **ambos acuerdan** parar; MuSig2 suelta la boleta ahora. **E2E 8** ✅ **PASS** (E2E 8 cancel_after_p1)
49. P1 cobrada; C quiere hacer P2; M no fondea P2 → C espera y recupera boleta. ✅ **PASS** (= 47)
50. P1 cobrada; M quiere P2; C no quiere trabajar más → M no puede ejecutar la boleta; C la recupera al timeout. ✅ **PASS** (= 47)

---

## 6. P1 locked — política `unwind` (P2 aún no existe)

51. C no trabaja y se va → tras T_p1, P1→M; tras T_proyecto, boleta→C. **E2E 2** ✅ **PASS** (E2E 2 contractor_abandons)
52. C **sí** construyó el muro y M no firma recepción → **igual on-chain que 51**; C pierde el trabajo hundido. **E2E 3** ✅ **PASS** (= 51; obra hecha, M no firma = mismo on-chain)
53. Calidad mala (a juicio de M); M no firma → otra vez igual que 51. ✅ **PASS** (= 51)
54. Ambos enojados, nadie firma → unwind de cada UTXO al dueño original. **E2E 9** (= E2E 2) ✅ **PASS** (E2E 9 = E2E 2)
55. Cancelación cooperativa **antes** de trabajar: P1→M, boleta→C. **E2E 4** ✅ **PASS** (E2E 4 coop_cancel)
56. Cancelación cooperativa **después** de haber trabajado un poco (se van de acuerdo): igual on-chain que 55. ✅ **PASS** (= 55)
57. Split acordado de P1 (p.ej. pintura 80/20) y boleta se libera. **E2E 6** (el demo lo hace en P2) ✅ **PASS** (E2E 6 split_80 (demo en P2))
58. Split de P1 y **después** pelean la boleta → boleta al timeout a C. ✅ **PASS** (= 47; split luego boleta al timeout)
59. M intenta `unwind --kind bond` → rechazo (no es boleta bancaria). **E2E 2/4** ✅ **PASS** (E2E 2/4 mandante no unwind boleta)
60. C intenta `unwind --kind partida` → rechazo. **E2E 2/4** ✅ **PASS** (E2E 2/4 contratista no unwind partida)
61. Unwind **antes** de T (CLTV / MTP) → la red no la mina. ✅ **PASS** (regtest_catalog unwind_before_T)
62. C termina tarde; M **igual** firma (coop post-T). ✅ **PASS** (= 46; coop post-T = key path)
63. C termina tarde; M ya unwindió P1. ✅ **PASS** (= 51)
64. Retraso por clima/fuerza mayor: el protocolo no lo sabe; o coop cancel o el reloj. ✅ **PASS** (= 55; fuerza mayor = coop o reloj)

---

## 7. P1 ok, P2 locked — política `unwind`

65. P2 cobrada y boleta liberada. **E2E 1** ✅ **PASS** (E2E 1)
66. C abandona P2 → P2→M tras T_p2; boleta→C tras T_proyecto; P1 ya cobrada se queda en C. ✅ **PASS** (regtest_catalog p2_abandon)
67. P2 hecha y M no firma → igual on-chain que 66 (trabajo hundido de P2). ✅ **PASS** (= 66)
68. Cancel coop de P2 (reembolso a M) y boleta liberada ahora. ✅ **PASS** (= 48; cancel P2+boleta ahora)
69. Cancel coop de P2 y boleta al timeout. ✅ **PASS** (= 66; cancel P2 y boleta timeout)
70. Split de P2 y boleta liberada. **E2E 6** ✅ **PASS** (E2E 6)
71. Split de P2 y boleta al timeout. ✅ **PASS** (= 66)
72. Ambos enojados en P2, nadie firma. ✅ **PASS** (= 66)
73. M fondea P2 tan tarde que T_p2 está encima → M puede unwind casi de inmediato. ✅ **PASS** (= 66; T_p2 encima)
74. `T_proyecto` vence con P2 todavía locked: UTXOs independientes (boleta la barre C; P2 la barre M al suyo). ✅ **PASS** (= 66; UTXOs independientes)

---

## 8. MAD (tercera salida; no se mezcla con árbitro)

El “quema” no es automático: tras T el key path **sigue**. NUMS solo impide que uno solo barre. Si nadie coopera nunca, el UTXO queda improductivo.

75. Feliz: P1+P2+boleta coop; MAD 50/50 (o cada uno recupera su mitad). **código** ✅ **PASS** (regtest_catalog mad_split_50_50)
76. Feliz: MAD se la queda uno, por acuerdo. **código** ✅ **PASS** (= 75; MAD a uno = pay_sats distinto; misma tx tipo)
77. Cancel coop de todo, MAD se devuelve. ✅ **PASS** (= 75)
78. P1 unwind, P2 no se fondea; MAD se reparte igual (se hacen los locos con el stake). ✅ **PASS** (= 79)
79. P1 unwind; nadie toca MAD nunca → improductivo. ✅ **PASS** (regtest_catalog mad_left_unspent)
80. Ambos enojados: P1 unwind, boleta unwind, MAD improductivo. ✅ **PASS** (regtest_catalog mad_burn_both_angry)
81. P2 en disputa unwind; MAD improductivo. ✅ **PASS** (= 80)
82. Split 80/20 en P2 y MAD 50/50 (la obra se negocia; el palo no). ✅ **PASS** (= 75)
83. Split en P2 y MAD se deja morir (despecho). ✅ **PASS** (= 79)
84. Uno quiere repartir MAD, el otro no → si no hay dos firmas, improductivo. ✅ **PASS** (= 79)
85. Después de T se reconcilian y MuSig2 de MAD (evitan la quema). ✅ **PASS** (= 75; MAD coop después de T: key path)
86. Alguien intenta gastar la hoja NUMS → imposible. **código** ✅ **PASS** (unit mad_leaf_is_nums)
87. Spite: la obra se cerró bien y aun así dejan morir MAD. ✅ **PASS** (= 79)

Nunca a una wallet del autor del software.

---

## 9. Árbitro — qué puede y qué no (vale para P1, P2 y boleta)

Hasta T, A no puede nada. A **solo** no puede vaciar. Tras T: `A&&M` o `A&&C` (el tx puede pagar a M, a C, split, o incluso una fee a A). Tras T2 = T+ventana, **además** vuelve el unwind unilateral. Tras T2 **siguen vivas** las hojas A+M / A+C: hay carrera.

88. A actúa a tiempo (entre T y T2). ✅ **PASS** (regtest_catalog a_on_time)
89. A actúa tarde pero **antes** de que el unwind se mine. ✅ **PASS** (= 88)
90. A actúa **después** de T2 y pierde la carrera contra el unwind. ✅ **PASS** (regtest_catalog a_too_late_unwind_wins)
91. A intenta firmar **antes** de T → inválido. ✅ **PASS** (regtest_catalog ac_before_T_rejected)
92. A firma solo → inválido. ✅ **PASS** (regtest_catalog a_not_a_party)
93. Tras T, las partes se reconcilian y usan MuSig2; A sobra. ✅ **PASS** (regtest_catalog reconcilia_musig_despues)

---

## 10. Árbitro en P1 (P2 aún no fondeada)

94. A da P1 a C (obra hecha) con `A&&C`. ✅ **PASS** (regtest_catalog ac_awards_p1_to_c)
95. A da P1 a M (no hecha / no conforme) con `A&&M`. ✅ **PASS** (regtest_catalog am_refunds_p1)
96. A impone un split (p.ej. 80/20): hace falta que **A y el favorecido** firmen ese split. ✅ **PASS** (regtest_catalog ac_split_80)
97. A dice 80 % a C; C quiere 100 % y no firma → si llega T2, M unwind 100 % (C empeora). ✅ **PASS** (= 99; C no firma el split → T2 unwind)
98. A dice 80 % a C; M no quiere pagar nada; `A&&C` igual ejecuta el 80/20 (M no puede pararlo tras T). ✅ **PASS** (regtest_catalog m_cannot_stop_ac)
99. A desaparece / no contesta → T2: P1→M, más tarde boleta→C. ✅ **PASS** (regtest_catalog a_disappears_t2)
100. A “opina” por WhatsApp y **nunca firma el PSBT** → igual que desaparecer. ✅ **PASS** (= 99)
101. A no sabe firmar PSBT → desaparecer. ✅ **PASS** (= 99)
102. A pierde las keys o muere → desaparecer. ✅ **PASS** (= 99)
103. A está en California y no opina sobre el muro → desaparecer. ✅ **PASS** (= 99)
104. A se demora hasta T2; M ya unwindió P1. ✅ **PASS** (= 90)
105. A es amigo de M y siempre firma `A&&M` (C no debió aceptar a esa persona). ✅ **PASS** (= 95; A sesgado a M = siempre A+M)
106. A es amigo de C y siempre firma `A&&C`. ✅ **PASS** (= 94; A sesgado a C = siempre A+C)
107. A pide fee on-chain (salida para A en el tx `A&&C` o `A&&M`). ✅ **PASS** (= 96; fee a A = extra output; split cubre 2 dest)
108. A pide plata off-chain para firmar. ❌ **NO TEST** (coima off-chain)
109. A firma **las dos** versiones (P1 a M y P1 a C) → carrera en mempool; A deshonesto. ❌ **NO TEST** (A firma las dos txs: carrera mempool no automatizada)
110. A+M mandan P1 a un tercero (ambos pueden). ✅ **PASS** (= 95; A+M eligen dest)

---

## 11. Árbitro en P2 (P1 ya cobrada)

111. A da P2 a C; boleta se libera en coop. ✅ **PASS** (= 94; P2 en vez de P1; misma hoja A+C)
112. A da P2 a M (reembolso); boleta coop a C. ✅ **PASS** (= 95)
113. A da P2 a M **y** da la boleta a M (castigo tipo boleta bancaria). ✅ **PASS** (= 120)
114. A da P2 a M y **no toca** la boleta → C la unwind en T2 de proyecto. ✅ **PASS** (= 99)
115. A desaparece en P2: P2→M al T2 de P2; boleta→C al T2 de proyecto. ✅ **PASS** (= 99)
116. A resuelve P2 y desaparece para la boleta (desaparición parcial). ✅ **PASS** (= 99)
117. A resuelve la boleta y no P2. ✅ **PASS** (= 120)
118. A da P1 a C y P2 a M (calidad distinta en cada hito). ✅ **PASS** (= 94; P1 a C; P2 a M = 94+95)
119. P2 se fondea con T_p2 ya encima; A apenas tiene ventana. ✅ **PASS** (= 91)

---

## 12. Árbitro y la boleta (esto reabre la “boleta bancaria”)

Sin A, M **nunca** se lleva la boleta. Con A, tras `T_proyecto`, `A&&M` **sí puede**.

120. C abandonó; A da la boleta a M (equivalente a ejecutar boleta de banco). ✅ **PASS** (regtest_catalog am_seizes_bond)
121. C abandonó; A se niega; T2: boleta→C (vuelve el default unwind). ✅ **PASS** (= 99; A se niega ≡ desaparece en boleta)
122. Obra hecha y M quiere la boleta igual; A honesto se niega; C la recupera. ✅ **PASS** (= 99)
123. Obra hecha; A capturado por M; boleta se va a M. ✅ **PASS** (= 120)
124. A desaparece en la boleta → C unwind en T2. ✅ **PASS** (= 99)
125. Tras T2, carrera: C unwind boleta vs `A&&M` hacia M. ❌ **NO TEST** (carrera T2 C-unwind vs A+M no minada aparte)
126. Tras T2, carrera en P2: M unwind P2 vs `A&&C` hacia C. ❌ **NO TEST** (carrera T2 M-unwind vs A+C no minada aparte)

---

## 13. Keys, archivos, fees, cadena

127. M pierde la key **antes** de T_p1: ni coop ni unwind de P1 (el unwind de partida es de M). P1 puede morir. ❌ **NO TEST** (pérdida de key M no script)
128. C pierde la key: boleta no se coop-libera ni se unwind (el unwind de boleta es de C). Boleta puede morir. ❌ **NO TEST** (pérdida de key C no script)
129. Ambos pierden keys → ambos UTXO muertos. ❌ **NO TEST** (ambas keys)
130. Herederos tienen las keys / no las tienen (muerte de M o C). ❌ **NO TEST** (herederos)
131. Reuso de nonce MuSig2 → abort (protege la key). ✅ **PASS** (unit nonce reuse)
132. Se pierde `nonces.json` o `identity.json` o `state.json` con UTXO vivos. ❌ **NO TEST** (archivos perdidos con UTXO vivos)
133. Fee de mercado a 6 meses: el unwind se firma al vencer (no hay tx prefirmada podrida). ✅ **PASS** (= 51; unwind se firma al vencer)
134. Coop/unwind stuck; hay que RBF. ❌ **NO TEST** (RBF no automatizado)
135. Unwind emitido con wall-clock pero MTP de BIP-113 aún no → mempool la rechaza. ✅ **PASS** (regtest_catalog unwind_before_T / MTP)
136. Reorg de una recepción o de un unwind. ❌ **NO TEST** (reorg recepción)
137. Cobra P2 a una address personal de C, no al Taproot (fuera de protocolo). ❌ **NO TEST** (pago a address personal fuera de protocolo)

---

## 14. Fuera del script, pero cambian el resultado humano

138. C adelanta P2 en el terreno sin que P2 esté fondeada. ❌ **NO TEST** (C adelanta obra)
139. Evidencia (fotos, testigos) que A usa o ignora; la cadena no la ve. ❌ **NO TEST** (fotos/testigos off-chain)
140. Injunction legal off-chain; el reloj on-chain sigue. ❌ **NO TEST** (injunction legal)
141. Intentan MAD+árbitro a la vez → fuera del MVP. ✅ **PASS** (cli_catalog mad_xor_arbiter)
142. Quieren 3+ partidas, boleta que rueda, o re-quote de P2 → no está. ✅ **PASS** (offer valida 2 partidas y plazos; no hay re-quote P2)

---

## Correspondencia con E2E minado

| Catálogo | E2E | Script |
|---|---|---|
| 42, 31, 65 | 1 feliz | `regtest_happy_path.sh` |
| 51, 54 | 2 abandono / 9 ambos enojados | `regtest_contractor_abandons.sh` |
| 52 | 3 obra hecha, M no firma (= 2 on-chain) | el de #2 |
| 55 | 4 cancel coop | `regtest_coop_cancel.sh` |
| 47 | 5 para tras P1 | `regtest_stops_after_partida1.sh` |
| 70 (57 en P1) | 6 split 80/20 | `regtest_split_80.sh` |
| 33 | 7 sin boleta | `regtest_no_bond.sh` |
| 48 | 8 cancel acordado tras P1 | `regtest_cancel_after_p1.sh` |
| 59, 60 | robos rechazados por CLI | chequeo en #2 y #4 |

Huecos que más se repiten:

- **52 = 51 on-chain.** El protocolo no distingue “no trabajó” de “trabajó y M no firma”.
- **120** es el único camino en el que M puede llevarse la boleta: hace falta A que coopera. Si A desaparece (**99, 115, 124**), eso no pasa: vuelve el unwind.
