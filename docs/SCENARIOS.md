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

---

## 0. Identidad y offer (nada on-chain)

1. M no publica nada.
2. M publica `unwind`; C acepta; M hace `commit`.
3. Igual con `mad`.
4. Igual con `arbiter` (solo el *slot*, sin persona).
5. C rechaza el offer y se va.
6. C acepta pero cambia plazos, montos, `bond_bps` o la política → `commit` rechaza.
7. C acepta; M nunca contrafirma.
8. M contrafirma; C nunca hace `import`.
9. M y C usan la misma pubkey.
10. Red distinta (regtest vs signet).
11. Offer con 0 partidas, monto 0, plazo no-unix, o P2 después de `T_proyecto`.
12. Archivo `00-offer.json` se pierde o se corrompe.

---

## 1. Quote (sats de boleta, P1 y P2 quedan fijos)

13. Ambos firman el quote.
14. M cotiza; C nunca `accept-quote`.
15. C cotiza; M nunca contrafirma.
16. Firma del peer inválida o montos alterados → rechazo (`hbp-quote`).
17. BTC se mueve **entre quote y fondeo de P1**: fondean igual o no fondean.
18. BTC se mueve **entre P1 cobrada y fondeo de P2**: el protocolo **no** re-cotiza P2; o pagan los sats viejos o no hay P2.
19. Boleta en USD ya no es el 10 % por volatilidad; igual está locked en sats.
20. `mad_bps` 0, >5 % o `mad_sats` bajo dust → rechazo.

---

## 2. Nombrar árbitro (solo si la política es `arbiter`)

21. Ambos firman el mismo A (alguien del pueblo que sabe de obra y de PSBT). **código**
22. M propone a un amigo; C no firma → no hay addresses, no hay UTXO. **código**
23. C propone a A1, M propone a A2 → deadlock.
24. Intentan que A sea M o C → rechazo. **código**
25. Nunca proponen a nadie. **código** (`hbp addresses` no imprime boleta/partida)
26. Una sola firma en `03-arbiter.json`; la otra no llega.
27. Quieren cambiar a A después de las dos firmas, antes de fondear → locked. **código**
28. Quieren nombrar a A **después** de fondear → imposible (cambiaría la address). **código**
29. Eligen a un admin de Bitcoin en California que no verá el muro (A inútil de entrada).
30. Eligen al alcalde local que no sabe firmar un PSBT (A inútil de otra manera).

---

## 3. Fondeo (boleta + P1 atómico; P2 después)

31. Fondeo correcto boleta+P1. **E2E 1**
32. C nunca pone boleta; M no manda P1 → nada locked.
33. C nunca pone boleta; M manda P1 igual → `verify-funding` rechaza; si bypass, P1 se unwind a M tras T. **E2E 7**
34. Tx sin salida de boleta, o monto de P1 distinto al quote, o output sorpresa.
35. Política MAD sin tercera salida `2*mad_sats`, o monto MAD mal. **código**
36. Solo fondean MAD, o solo boleta, o solo P1.
37. Fondean P2 antes de que P1 esté terminal → rechazo.
38. Fondean a la address **equivocada** (árbol unwind vs árbol con A) → la otra punta no verifica; los sats pueden quedar irrecuperables.
39. Reorg de la tx de fondeo.
40. C empieza a trabajar **antes** de confirmaciones / antes de P1 locked.
41. Política arbiter pero fondean como si fuera unwind (A no está en el árbol).

---

## 4. Camino feliz (A no actúa; MAD no quema)

42. `unwind`: P1 cobrada, P2 cobrada, boleta liberada (MuSig2 × 3). **E2E 1**
43. `mad`: igual + MAD se devuelve/reparte en coop. **código**
44. `arbiter`: igual; A nunca firma nada (hoja ciega). **código** (descriptors; falta E2E)
45. P1 y P2 se reciben **antes** de sus T (obra temprana).
46. P1 se recibe **después** de T_p1 porque M aún no unwindió (el key path nunca caduca).

---

## 5. Parar el proyecto tras P1 (P2 nunca se fondea)

47. P1 cobrada; M no sigue; espera `T_proyecto`; C unwind boleta. **E2E 5**
48. P1 cobrada; **ambos acuerdan** parar; MuSig2 suelta la boleta ahora. **E2E 8**
49. P1 cobrada; C quiere hacer P2; M no fondea P2 → C espera y recupera boleta.
50. P1 cobrada; M quiere P2; C no quiere trabajar más → M no puede ejecutar la boleta; C la recupera al timeout.

---

## 6. P1 locked — política `unwind` (P2 aún no existe)

51. C no trabaja y se va → tras T_p1, P1→M; tras T_proyecto, boleta→C. **E2E 2**
52. C **sí** construyó el muro y M no firma recepción → **igual on-chain que 51**; C pierde el trabajo hundido. **E2E 3**
53. Calidad mala (a juicio de M); M no firma → otra vez igual que 51.
54. Ambos enojados, nadie firma → unwind de cada UTXO al dueño original. **E2E 9** (= E2E 2)
55. Cancelación cooperativa **antes** de trabajar: P1→M, boleta→C. **E2E 4**
56. Cancelación cooperativa **después** de haber trabajado un poco (se van de acuerdo): igual on-chain que 55.
57. Split acordado de P1 (p.ej. pintura 80/20) y boleta se libera. **E2E 6** (el demo lo hace en P2)
58. Split de P1 y **después** pelean la boleta → boleta al timeout a C.
59. M intenta `unwind --kind bond` → rechazo (no es boleta bancaria). **E2E 2/4**
60. C intenta `unwind --kind partida` → rechazo. **E2E 2/4**
61. Unwind **antes** de T (CLTV / MTP) → la red no la mina.
62. C termina tarde; M **igual** firma (coop post-T).
63. C termina tarde; M ya unwindió P1.
64. Retraso por clima/fuerza mayor: el protocolo no lo sabe; o coop cancel o el reloj.

---

## 7. P1 ok, P2 locked — política `unwind`

65. P2 cobrada y boleta liberada. **E2E 1**
66. C abandona P2 → P2→M tras T_p2; boleta→C tras T_proyecto; P1 ya cobrada se queda en C.
67. P2 hecha y M no firma → igual on-chain que 66 (trabajo hundido de P2).
68. Cancel coop de P2 (reembolso a M) y boleta liberada ahora.
69. Cancel coop de P2 y boleta al timeout.
70. Split de P2 y boleta liberada. **E2E 6**
71. Split de P2 y boleta al timeout.
72. Ambos enojados en P2, nadie firma.
73. M fondea P2 tan tarde que T_p2 está encima → M puede unwind casi de inmediato.
74. `T_proyecto` vence con P2 todavía locked: UTXOs independientes (boleta la barre C; P2 la barre M al suyo).

---

## 8. MAD (tercera salida; no se mezcla con árbitro)

El “quema” no es automático: tras T el key path **sigue**. NUMS solo impide que uno solo barre. Si nadie coopera nunca, el UTXO queda improductivo.

75. Feliz: P1+P2+boleta coop; MAD 50/50 (o cada uno recupera su mitad). **código**
76. Feliz: MAD se la queda uno, por acuerdo. **código**
77. Cancel coop de todo, MAD se devuelve.
78. P1 unwind, P2 no se fondea; MAD se reparte igual (se hacen los locos con el stake).
79. P1 unwind; nadie toca MAD nunca → improductivo.
80. Ambos enojados: P1 unwind, boleta unwind, MAD improductivo.
81. P2 en disputa unwind; MAD improductivo.
82. Split 80/20 en P2 y MAD 50/50 (la obra se negocia; el palo no).
83. Split en P2 y MAD se deja morir (despecho).
84. Uno quiere repartir MAD, el otro no → si no hay dos firmas, improductivo.
85. Después de T se reconcilian y MuSig2 de MAD (evitan la quema).
86. Alguien intenta gastar la hoja NUMS → imposible. **código**
87. Spite: la obra se cerró bien y aun así dejan morir MAD.

Nunca a una wallet del autor del software.

---

## 9. Árbitro — qué puede y qué no (vale para P1, P2 y boleta)

Hasta T, A no puede nada. A **solo** no puede vaciar. Tras T: `A&&M` o `A&&C` (el tx puede pagar a M, a C, split, o incluso una fee a A). Tras T2 = T+ventana, **además** vuelve el unwind unilateral. Tras T2 **siguen vivas** las hojas A+M / A+C: hay carrera.

88. A actúa a tiempo (entre T y T2).
89. A actúa tarde pero **antes** de que el unwind se mine.
90. A actúa **después** de T2 y pierde la carrera contra el unwind.
91. A intenta firmar **antes** de T → inválido.
92. A firma solo → inválido.
93. Tras T, las partes se reconcilian y usan MuSig2; A sobra.

---

## 10. Árbitro en P1 (P2 aún no fondeada)

94. A da P1 a C (obra hecha) con `A&&C`.
95. A da P1 a M (no hecha / no conforme) con `A&&M`.
96. A impone un split (p.ej. 80/20): hace falta que **A y el favorecido** firmen ese split.
97. A dice 80 % a C; C quiere 100 % y no firma → si llega T2, M unwind 100 % (C empeora).
98. A dice 80 % a C; M no quiere pagar nada; `A&&C` igual ejecuta el 80/20 (M no puede pararlo tras T).
99. A desaparece / no contesta → T2: P1→M, más tarde boleta→C.
100. A “opina” por WhatsApp y **nunca firma el PSBT** → igual que desaparecer.
101. A no sabe firmar PSBT → desaparecer.
102. A pierde las keys o muere → desaparecer.
103. A está en California y no opina sobre el muro → desaparecer.
104. A se demora hasta T2; M ya unwindió P1.
105. A es amigo de M y siempre firma `A&&M` (C no debió aceptar a esa persona).
106. A es amigo de C y siempre firma `A&&C`.
107. A pide fee on-chain (salida para A en el tx `A&&C` o `A&&M`).
108. A pide plata off-chain para firmar.
109. A firma **las dos** versiones (P1 a M y P1 a C) → carrera en mempool; A deshonesto.
110. A+M mandan P1 a un tercero (ambos pueden).

---

## 11. Árbitro en P2 (P1 ya cobrada)

111. A da P2 a C; boleta se libera en coop.
112. A da P2 a M (reembolso); boleta coop a C.
113. A da P2 a M **y** da la boleta a M (castigo tipo boleta bancaria).
114. A da P2 a M y **no toca** la boleta → C la unwind en T2 de proyecto.
115. A desaparece en P2: P2→M al T2 de P2; boleta→C al T2 de proyecto.
116. A resuelve P2 y desaparece para la boleta (desaparición parcial).
117. A resuelve la boleta y no P2.
118. A da P1 a C y P2 a M (calidad distinta en cada hito).
119. P2 se fondea con T_p2 ya encima; A apenas tiene ventana.

---

## 12. Árbitro y la boleta (esto reabre la “boleta bancaria”)

Sin A, M **nunca** se lleva la boleta. Con A, tras `T_proyecto`, `A&&M` **sí puede**.

120. C abandonó; A da la boleta a M (equivalente a ejecutar boleta de banco).
121. C abandonó; A se niega; T2: boleta→C (vuelve el default unwind).
122. Obra hecha y M quiere la boleta igual; A honesto se niega; C la recupera.
123. Obra hecha; A capturado por M; boleta se va a M.
124. A desaparece en la boleta → C unwind en T2.
125. Tras T2, carrera: C unwind boleta vs `A&&M` hacia M.
126. Tras T2, carrera en P2: M unwind P2 vs `A&&C` hacia C.

---

## 13. Keys, archivos, fees, cadena

127. M pierde la key **antes** de T_p1: ni coop ni unwind de P1 (el unwind de partida es de M). P1 puede morir.
128. C pierde la key: boleta no se coop-libera ni se unwind (el unwind de boleta es de C). Boleta puede morir.
129. Ambos pierden keys → ambos UTXO muertos.
130. Herederos tienen las keys / no las tienen (muerte de M o C).
131. Reuso de nonce MuSig2 → abort (protege la key).
132. Se pierde `nonces.json` o `identity.json` o `state.json` con UTXO vivos.
133. Fee de mercado a 6 meses: el unwind se firma al vencer (no hay tx prefirmada podrida).
134. Coop/unwind stuck; hay que RBF.
135. Unwind emitido con wall-clock pero MTP de BIP-113 aún no → mempool la rechaza.
136. Reorg de una recepción o de un unwind.
137. Cobra P2 a una address personal de C, no al Taproot (fuera de protocolo).

---

## 14. Fuera del script, pero cambian el resultado humano

138. C adelanta P2 en el terreno sin que P2 esté fondeada.
139. Evidencia (fotos, testigos) que A usa o ignora; la cadena no la ve.
140. Injunction legal off-chain; el reloj on-chain sigue.
141. Intentan MAD+árbitro a la vez → fuera del MVP.
142. Quieren 3+ partidas, boleta que rueda, o re-quote de P2 → no está.

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
