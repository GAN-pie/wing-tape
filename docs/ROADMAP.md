# Roadmap — Détecteur d'oiseaux embarqué (BirdNET sur Raspberry Pi)

| Champ | Valeur |
|---|---|
| **Version du document** | 1.0 |
| **Dernière mise à jour** | 19 octobre 2026 |
| **Type de projet** | Side project personnel, open source |
| **Statut global** | Phase 0 terminée, Phase 1 à démarrer |
| **Cadence estimée** | ~6–8 h / semaine |

---

## 1. Objectif et périmètre

**Objectif final** : disposer d'un dispositif autonome capable de capturer de l'audio en continu, de détecter et identifier les chants d'oiseaux en temps réel via BirdNET, d'enregistrer les détections de manière structurée, et de fonctionner sur batterie pendant une durée mesurée et documentée.

**Dans le périmètre**
- Capture audio temps réel et inférence locale sur Raspberry Pi 3
- Modèle BirdNET en version standard (pas de fine-tuning)
- Windowing 3 s avec recouvrement de 1 s (rafraîchissement toutes les 2 s)
- Journalisation locale des détections
- Alimentation sur batterie, avec campagne de mesure d'autonomie
- Documentation publique à chaque jalon

**Hors périmètre (reporté)**
- Fine-tuning sur des espèces locales
- Alimentation solaire et gestion de relais
- Interface utilisateur graphique ou application mobile
- Déploiement multi-nœuds / réseau de capteurs
- Transmission distante temps réel (API, MQTT)

---

## 2. Principes de gestion

**Cadence** : les estimations sont volontairement grossières. Elles servent à situer l'avancement, pas à contraindre. Chaque phase est estimée en *effort* (jours-homme) et en *calendrier indicatif* (semaines réelles à ~6–8 h/semaine).

**Définition de « terminé »** pour toute tâche :
1. Le code fonctionne de manière reproductible
2. Une mesure ou un résultat observable le confirme
3. C'est documenté (commentaire, note, ou section de doc)
4. C'est committé avec un message explicite

**Règle de jalon** : un jalon n'est franchi que si son **critère de sortie** est vérifié. Un jalon franchi déclenche systématiquement une publication (article, note de blog, README, post communautaire).

**Légende**
| Symbole | Signification |
|---|---|
| ✅ | Terminé |
| 🔄 | En cours |
| ⬜ | À faire |
| ⛔ | Bloquant pour la suite |
| 🔗 | Dépendance amont |
| 📣 | Livrable de publication |

---

## 3. Vue d'ensemble des phases

| Phase | Intitulé | Effort estimé | Calendrier | Statut |
|---|---|---|---|---|
| 0 | Fondations techniques | — | — | ✅ Terminée |
| 1 | Intégration capture ↔ inférence | 5–8 j-h | 2–3 sem. | ⬜ À démarrer |
| 2 | Validation sur données réelles | 4–6 j-h | 2–3 sem. | ⬜ |
| 3 | Post-traitement et journalisation | 4–6 j-h | 2 sem. | ⬜ |
| 4 | Profilage et optimisation | 5–7 j-h | 2–3 sem. | ⬜ |
| 5 | Autonomie sur batterie | 4–6 j-h | 3–5 sem. | ⬜ |
| T | Chantier transverse : documentation | 1–2 j-h / phase | continu | 🔄 |
| **Total** | | **~25–35 j-h** | **~3–4 mois** | |

---

## 4. Détail des phases

### Phase 0 — Fondations techniques ✅

| ID | Tâche | Statut | Livrable |
|---|---|---|---|
| T0.1 | Module de capture audio temps réel | ✅ | Composant fonctionnel et débogué |
| T0.2 | Moteur d'inférence BirdNET (runtime embarqué) | ✅ | Inférence opérationnelle sur données synthétiques |
| T0.3 | Choix de la pile technique (modèle, runtime, bindings) | 🔄 | Note d'architecture à figer et documenter |

**Critère de sortie atteint** : les deux briques fonctionnent indépendamment. Aucune intégration n'existe encore.

**Point d'attention** : T0.3 doit être formellement clos avant T1.1, car le contrat de données dépend du modèle et du runtime retenus. Documenter ce choix (version du modèle, quantification, nature de l'entrée attendue) est indispensable pour la reproductibilité par des tiers.

---

### Phase 1 — Intégration capture ↔ inférence

**Objectif** : obtenir une boucle fermée où l'audio capté est analysé automatiquement, en continu, sans intervention manuelle.

| ID | Tâche | Dépend de | Statut |
|---|---|---|---|
| T1.1 | Figer les contrats d'interface (format audio attendu, format du tenseur d'entrée) | T0.3 | ⬜ ⛔ |
| T1.2 | Implémenter la fenêtre glissante (3 s, recouvrement 1 s) | T1.1 | ⬜ |
| T1.3 | Mettre en place le pipeline à deux flux séparés (capture / inférence) | T1.1 | ⬜ |
| T1.4 | Définir la politique de traitement en cas de retard d'inférence | T1.3 | ⬜ |
| T1.5 | Abstraire la source audio (micro réel / fichier de test) | T1.1 | ⬜ ⛔ |
| T1.6 | Boucle d'exécution continue avec arrêt propre | T1.2, T1.3, T1.4 | ⬜ |
| T1.7 | Journalisation brute minimale (timestamp, scores) | T1.6 | ⬜ |

**Livrables** : exécutable tournant en continu, logs de détection horodatés, mesures de temps d'inférence brutes.

**Critère de sortie (Jalon J1)** : le système tourne 30 minutes sans interruption, produit des scores sur l'audio réel capté, et accepte indifféremment un fichier de test ou le micro via la même interface.

📣 **Publication J1** : note d'architecture (schéma de flux, justification du découpage en deux flux) + extrait de code minimal.

---

### Phase 2 — Validation sur données réelles

**Objectif** : prouver que la chaîne complète produit des résultats corrects sur de vrais chants d'oiseaux.

| ID | Tâche | Dépend de | Statut |
|---|---|---|---|
| T2.1 | Constituer un corpus de référence (enregistrements connus, espèces variées) | — | ⬜ *(peut démarrer en parallèle de la Phase 1)* |
| T2.2 | Harnais de test d'équivalence : comparer la sortie du pipeline avec une référence Python | T1.5, T2.1 | ⬜ ⛔ |
| T2.3 | Campagne de tests sur 10–20 espèces | T2.2 | ⬜ |
| T2.4 | Analyser les écarts, faux positifs et faux négatifs | T2.3 | ⬜ |
| T2.5 | Premiers enregistrements en conditions réelles (extérieur) | T2.3 | ⬜ |

**Livrables** : tableau de validation par espèce, rapport de divergence, premiers résultats terrain.

**Critère de sortie (Jalon J2)** : le pipeline reproduit fidèlement les résultats de la référence sur le corpus, et détecte correctement au moins une espèce sur un enregistrement terrain.

📣 **Publication J2** : « Premières détections réelles » — tableau comparatif attendu/obtenu, exemples de succès et d'échecs, limites observées.

---

### Phase 3 — Post-traitement et journalisation

**Objectif** : transformer des scores bruts en détections fiables et exploitables.

| ID | Tâche | Dépend de | Statut |
|---|---|---|---|
| T3.1 | Définir et régler le seuil de confiance | T2.4 | ⬜ |
| T3.2 | Lissage temporel (confirmation d'une détection sur plusieurs cycles) | T3.1 | ⬜ |
| T3.3 | Journalisation structurée persistante (base locale) | T1.7 | ⬜ ⛔ |
| T3.4 | Horodatage fiable et stable sur longue durée | T3.3 | ⬜ |
| T3.5 | Outil de consultation / export des détections | T3.3 | ⬜ |
| T3.6 | Génération d'un rapport de session (synthèse par espèce, période) | T3.3, T3.5 | ⬜ |

**Livrables** : base de détections structurée, filtrage des faux positifs opérationnel, moyen de consultation.

**Critère de sortie (Jalon J3)** : une session de plusieurs heures produit un jeu de données propre, exploitable par un tiers sans explication supplémentaire.

📣 **Publication J3** : guide « Interpréter les scores BirdNET » + exemple de jeu de données anonymisé.

---

### Phase 4 — Profilage et optimisation

**Objectif** : connaître précisément le comportement du système sous charge et le rendre stable sur la durée.

| ID | Tâche | Dépend de | Statut |
|---|---|---|---|
| T4.1 | Instrumenter le système (temps d'inférence, trames perdues, charge CPU, mémoire, température) | T3.4 | ⬜ ⛔ |
| T4.2 | Campagne de mesures courte durée (conditions nominales) | T4.1 | ⬜ |
| T4.3 | Analyse du budget temps disponible vs consommé | T4.2 | ⬜ |
| T4.4 | Optimisations ciblées (selon les goulets identifiés) | T4.3 | ⬜ |
| T4.5 | Test de stabilité longue durée (24–72 h en continu) | T4.4 | ⬜ |
| T4.6 | Mesurer la consommation électrique réelle du dispositif en fonctionnement | T4.2 | ⬜ ⛔ |

**Livrables** : graphes de charge, budget temps par composant, relevé de consommation électrique, rapport de stabilité.

**Critère de sortie (Jalon J4)** : le système tourne 72 h sans intervention, sans dégradation, et sa consommation électrique est chiffrée par état de fonctionnement.

📣 **Publication J4** : benchmark « BirdNET sur Raspberry Pi 3 » — métriques réalistes, enseignements d'optimisation.

---

### Phase 5 — Autonomie sur batterie

> **Périmètre révisé** : uniquement l'alimentation par batterie. Pas d'étude solaire, pas de relais, pas de dimensionnement photovoltaïque à ce stade.

**Objectif** : déterminer expérimentalement l'autonomie réelle du dispositif sur batterie, et juger si la configuration actuelle est suffisante ou non.

| ID | Tâche | Dépend de | Statut |
|---|---|---|---|
| T5.1 | Établir le profil énergétique par état (actif / veille / arrêt) | T4.6 | ⬜ ⛔ |
| T5.2 | Dimensionner les scénarios batterie (plusieurs capacités candidates) | T5.1 | ⬜ |
| T5.3 | Monter le banc de test (batterie + mesure de tension/courant) | T5.2 | ⬜ |
| T5.4 | Implémenter un mode économie d'énergie (activation sur seuil d'énergie audio) | T4.3 | ⬜ *(conditionnel : à activer seulement si l'autonomie mesurée est insuffisante)* |
| T5.5 | Campagnes d'observation longue durée sur batterie | T5.3 | ⬜ |
| T5.6 | Analyse d'autonomie et verdict : suffisant / insuffisant | T5.5 | ⬜ |
| T5.7 | Recommandations pour la suite (capacité supérieure, mode veille, ou report de l'autonomie) | T5.6 | ⬜ |

**Livrables** : profil énergétique, bilan d'autonomie chiffré, protocole de mesure reproductible, verdict argumenté.

**Critère de sortie (Jalon J5)** : une durée d'autonomie est mesurée et documentée pour au moins deux configurations de batterie, avec conclusion explicite sur la viabilité.

📣 **Publication J5** : « Autonomie d'un détecteur d'oiseaux sur batterie » — bilan complet, méthodologie de mesure, recommandations matérielles.

---

## 5. Chantier transverse — Documentation et diffusion

Ce chantier ne se termine jamais : il accompagne chaque phase.

| ID | Tâche | Rythme | Statut |
|---|---|---|---|
| TD.1 | Tenir un journal de bord daté (décisions, impasses, résultats) | Continu | 🔄 |
| TD.2 | Maintenir le dépôt structuré (README, doc d'architecture, licence) | Continu | ⬜ |
| TD.3 | Publier un article à chaque jalon franchi | Par jalon | ⬜ |
| TD.4 | Partager auprès des communautés concernées | Par jalon | ⬜ |
| TD.5 | Consolidation finale : guide complet reproductible | Après J5 | ⬜ |

**Canaux envisagés par audience**

| Audience | Moment | Canal |
|---|---|---|
| Développeurs embarqué / Rust | J1, J4 | Dépôt Git, forums techniques, Hackaday |
| Bioacousticiens / ornithologues | J2, J3 | Forums naturalistes, Xeno-Canto, iNaturalist |
| Makers / IoT | J4, J5 | Reddit r/raspberry_pi, forums maker |
| Écologie / conservation | J5 + TD.5 | Articles longs, présentation |

---

## 6. Matrice de dépendances

Lecture : la tâche en ligne dépend des tâches en colonne.

|  | T1.1 | T1.5 | T2.1 | T3.3 | T4.1 | T4.6 | T5.1 |
|---|---|---|---|---|---|---|---|
| T1.2 | ● | | | | | | |
| T1.3 | ● | | | | | | |
| T1.5 | ● | | | | | | |
| T2.2 | | ● | ● | | | | |
| T3.1 | | | | | | | |
| T3.3 | | | | | | | |
| T4.1 | | | | ● | | | |
| T4.6 | | | | | | | |
| T5.1 | | | | | | ● | |
| T5.4 | | | | | | | |

**Chaînes de dépendance principales**

- **T0.3 → T1.1 → T1.5 → T2.2 → T2.3 → T2.4 → T3.1 → T3.2**
- **T1.1 → T1.3 → T1.6 → T1.7 → T3.3 → T3.4 → T4.1 → T4.2 → T4.3 → T4.4 → T4.5**
- **T4.2 → T4.6 → T5.1 → T5.2 → T5.3 → T5.5 → T5.6 → T5.7**
- **T4.3 → T5.4** (branche conditionnelle, hors chemin critique)

---

## 7. Chemin critique

Le chemin critique traverse les tâches bloquantes (⛔) et les jalons :

```
T0.3 → T1.1 → T1.5 → T1.6 → T1.7 → J1
     → T2.2 → T2.3 → T2.4 → J2
     → T3.3 → T3.4 → J3
     → T4.1 → T4.2 → T4.6 → J4
     → T5.1 → T5.2 → T5.3 → T5.5 → T5.6 → J5
```

**Implications pratiques**

1. **T1.1 est le verrou initial.** Tant que les contrats d'interface ne sont pas figés, tout le reste de la Phase 1 est instable. C'est la tâche à traiter en premier, même si elle paraît administrative.

2. **T1.5 (abstraction de la source audio) est stratégique.** Elle conditionne T2.2, donc toute la validation. Sans elle, la Phase 2 devient un exercice de tests manuels non reproductibles.

3. **T3.3 et T3.4 sont des prérequis cachés de la Phase 5.** La mesure d'autonomie repose sur des sessions longues, donc sur une journalisation fiable et un horodatage stable. Sous-estimer la Phase 3 se paiera directement en Phase 5.

4. **T4.6 est le point de bascule vers la Phase 5.** Sans relevé de consommation électrique, le dimensionnement batterie se fait à l'aveugle.

5. **Les phases 2, 3 et 4 ne peuvent pas être parallélisées entre elles** : chacune consomme les résultats de la précédente. En revanche, **T2.1 (corpus) et TD.1–TD.2 sont parallélisables** dès maintenant.

6. **T5.4 est hors chemin critique.** C'est une variable d'ajustement : à n'activer que si le verdict de T5.6 l'exige. Ne pas l'anticiper.

---

## 8. Jalons et implications

| Jalon | Critère de sortie | Implications | Publication |
|---|---|---|---|
| **J1** | Système bout-en-bout tournant 30 min en continu | Débloque toute la validation ; première démo montrable | Note d'architecture + code |
| **J2** | Résultats conformes à la référence + 1 détection terrain | Le projet devient crédible scientifiquement | Article « premières détections » |
| **J3** | Session longue produisant un jeu de données propre | Le système devient utilisable par un tiers | Guide d'interprétation + dataset |
| **J4** | 72 h de stabilité + consommation chiffrée | Base factuelle pour la Phase 5 ; benchmark publiable | Benchmark Pi 3 |
| **J5** | Autonomie mesurée sur ≥ 2 configurations batterie | Verdict sur la viabilité du mode autonome | Bilan d'autonomie |
| **TF** | Guide complet reproductible | Le projet est réutilisable par la communauté | Consolidation finale |

---

## 9. Registre des risques

| Risque | Probabilité | Impact | Mitigation |
|---|---|---|---|
| Contrats d'interface mal figés → refonte en Phase 1 | Moyenne | Élevé | Traiter T1.1 en priorité, écrire la spécification avant le code |
| Validation impossible faute de harnais reproductible | Moyenne | Élevé | T1.5 et T2.2 obligatoires avant toute campagne de test |
| Performances insuffisantes sur Pi 3 | Moyenne | Moyen | Profilage précoce (T4.1), optimisations ciblées (T4.4) |
| Dérive du planning (side project) | Élevée | Faible | Estimations volontairement larges ; jalons plutôt que dates |
| Autonomie insuffisante sur batterie | Moyenne | Moyen | Verdict explicite en T5.6, recommandations en T5.7 ; mode économie en réserve (T5.4) |
| Perte de contexte entre sessions de travail | Élevée | Moyen | Journal de bord TD.1 tenu systématiquement |
| Documentation reportée en fin de projet | Moyenne | Élevé | Publication imposée à chaque jalon (règle de jalon) |

---

## 10. Rétro-planning indicatif

Hypothèse : ~6–8 h de travail par semaine. À ajuster librement.

| Semaine | Phase | Étape attendue | Jalon |
|---|---|---|---|
| S1 | 1 | T1.1, T1.2 | |
| S2 | 1 | T1.3, T1.4, T1.5 | |
| S3 | 1 | T1.6, T1.7 | **J1** |
| S4 | 2 | T2.1, T2.2 | |
| S5 | 2 | T2.3, T2.4 | |
| S6 | 2 | T2.5 | **J2** |
| S7 | 3 | T3.1, T3.2, T3.3 | |
| S8 | 3 | T3.4, T3.5, T3.6 | **J3** |
| S9 | 4 | T4.1, T4.2 | |
| S10 | 4 | T4.3, T4.4 | |
| S11 | 4 | T4.5, T4.6 | **J4** |
| S12 | 5 | T5.1, T5.2 | |
| S13 | 5 | T5.3 | |
| S14–S16 | 5 | T5.5 (campagnes longues) | |
| S17 | 5 | T5.6, T5.7 | **J5** |
| S18 | T | TD.5 | **TF** |

**Repères d'avancement** : à mi-parcours (~S8), les phases 0 à 3 doivent être closes. Un retard supérieur à 3 semaines sur ce repère signale un problème de périmètre plutôt que de vitesse — dans ce cas, réduire la Phase 5 avant de réduire la Phase 2.

---

## 11. Tableau de suivi

À cocher au fil de l'eau.

**Phase 1 — Intégration**
- [ ] T1.1 — Contrats d'interface figés
- [ ] T1.2 — Fenêtre glissante
- [ ] T1.3 — Pipeline à deux flux
- [ ] T1.4 — Politique de retard
- [ ] T1.5 — Abstraction source audio
- [ ] T1.6 — Boucle continue
- [ ] T1.7 — Journalisation brute
- [ ] **J1 franchi** + publication

**Phase 2 — Validation**
- [ ] T2.1 — Corpus de référence
- [ ] T2.2 — Harnais d'équivalence
- [ ] T2.3 — Campagne 10–20 espèces
- [ ] T2.4 — Analyse des écarts
- [ ] T2.5 — Enregistrements terrain
- [ ] **J2 franchi** + publication

**Phase 3 — Post-traitement**
- [ ] T3.1 — Seuil de confiance
- [ ] T3.2 — Lissage temporel
- [ ] T3.3 — Journalisation structurée
- [ ] T3.4 — Horodatage stable
- [ ] T3.5 — Consultation / export
- [ ] T3.6 — Rapport de session
- [ ] **J3 franchi** + publication

**Phase 4 — Profilage**
- [ ] T4.1 — Instrumentation
- [ ] T4.2 — Mesures nominales
- [ ] T4.3 — Analyse du budget temps
- [ ] T4.4 — Optimisations
- [ ] T4.5 — Stabilité 24–72 h
- [ ] T4.6 — Consommation électrique
- [ ] **J4 franchi** + publication

**Phase 5 — Autonomie batterie**
- [ ] T5.1 — Profil énergétique par état
- [ ] T5.2 — Scénarios batterie
- [ ] T5.3 — Banc de test
- [ ] T5.4 — Mode économie *(conditionnel)*
- [ ] T5.5 — Campagnes longues
- [ ] T5.6 — Verdict d'autonomie
- [ ] T5.7 — Recommandations
- [ ] **J5 franchi** + publication

**Transverse**
- [ ] TD.1 — Journal de bord à jour
- [ ] TD.2 — Dépôt structuré
- [ ] TD.3 — Articles publiés (0/5)
- [ ] TD.4 — Diffusion communautaire
- [ ] TD.5 — Guide final

---

## 12. Historique des révisions

| Version | Date | Modification |
|---|---|---|
| 1.0 | 19 oct. 2026 | Création. Phase 5 réduite à l'alimentation batterie (étude solaire retirée du périmètre). |

---

Quelques remarques sur cette structure, si tu veux l'ajuster :

Le point le plus utile à surveiller est **T1.5**. C'est une tâche d'apparence mineure qui détermine si ta Phase 2 sera une vraie validation ou une série de tests manuels impossibles à reproduire — et donc si tes publications de J2 auront une valeur scientifique ou seulement anecdotique.

Deuxième point : la Phase 3 est facile à bâcler parce qu'elle est peu gratifiante techniquement, mais c'est elle qui rend la Phase 5 mesurable. Si tu veux couper quelque part pour tenir ton planning, coupe dans la Phase 4 (les optimisations peuvent attendre) plutôt que dans la Phase 3.

Veux-tu que je détaille davantage un aspect — par exemple une grille de critères plus fine pour valider J2, ou un modèle de fiche à remplir pour chaque campagne de mesure de la Phase 5 ?
