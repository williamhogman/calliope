# 14 — Toponymy as Archaeology & the Geography of Situation

How real landscapes encode centuries of history in their names and their
settlement patterns — the layer of authenticity `06-culture-language.md`
(naming machinery) and `04-settlements-roads.md` (placement machinery)
stop short of. Theme: **a map should be readable as strata.**

## Sources

1. **English historical toponymy** — Institute for Name-Studies (Nottingham) VEPN; Kent Archaeological Society element glossaries; Hough, "Name Structures and Name Survival" (2016); SNSBI surveys — SKIM. England's names are discrete historical strata: Celtic river names (Thames, Avon — "avon" just means *river*), Roman *-chester/-caster*, Anglo-Saxon *-ton/-ham/-ford*, Norse *-by/-thorpe/-thwaite* — and the Norse layer is **geographically bounded** (the Danelaw), not scattered. Two structural laws: (a) **hydronym conservatism** — river names are the most linguistically conservative layer, surviving conquests that overwrite every settlement around them; (b) names are **compositional** — personal-name/descriptor + habitative suffix ("Nottingham" = Snot's people's homestead), so etymology is legible.
2. **Toponymic drift** — synthesis of the above — SKIM. Folk etymology reshapes foreign names into locally meaningful sounds; compounds erode with age. Simulatable: run the true etymon through phonological softening keyed to notional age — old cities read worn and opaque, new ones transparent.
3. **The Language Construction Kit** — Rosenfelder — https://zompist.com/kit.html — READ (cross-ref 06). The naming-language core: small phoneme inventory + syllable template + 5-10 recurring morphemes reused across a culture's territory, so patterns become recognizable — the signature of real language vs. letter salad.
4. **Site vs. situation; fall lines, portages, path dependence** — transportgeography.org; Bleakley & Lin, "Portage and Path Dependence" (QJE); Lin, "Geography, History, Economies of Density" — SKIM/ABSTRACT. *Site* = the spot (ford, hill, harbor); *situation* = the regional position (head of navigation, valley mouth). Fall-line cities form where navigable water ends; portage necks and confluences seed durable settlement; and — the deep result — cities **persist after their founding reason disappears** (path dependence). A town whose reason is obsolete is not a bug; it is history.
5. **Von Thünen rings; central place theory** — JASSS ABM verification; UCL teaching model — ABSTRACT. Transport cost alone produces concentric land-use rings around any market town (garden → crop → pasture → wild) and a nested size hierarchy of evenly spaced central places — cheap deterministic texture around every settlement.
6. **Martin O'Leary, "Generating Fantasy Maps"** — https://mewo2.com/notes/terrain/ + naming-language repo — READ (cross-ref 01/06/10). One shared per-culture generator for *all* names in a region; label styling varies by feature type. Cities scored on coast + confluence + flat land, roads grown along cost-minimizing paths.
7. **Here Dragons Abound** — Scott Turner — heredragonsabound.blogspot.com — SKIM (cross-ref 10). Imhof's label rules as multi-factor optimization; coastal/region labels curved along the feature; rivers as tapered polygons widened by accumulated flow. Feeds M7, listed here for the "surveyed, not stamped" feel it produces.
8. **Azgaar design blog; Undiscovered Worlds** — SKIM (cross-ref 01/04). Cost-weighted border diffusion from capitals (rivers/mountains as barriers) makes borders look fought-over; fixing physical scale early keeps climate, travel time and spacing dimensionally consistent — both already Calliope practice; cited as confirmation.

## Synthesis

Real geography feels old because it is **palimpsest**: the oldest language
layer survives on the rivers, conquest layers cluster in bounded zones on
the settlements, names erode in proportion to age, and towns outlive their
reasons. None of this needs new physical simulation — it needs the naming
and settlement systems to be given a *time dimension*: who named this,
in which era, and what has happened to the word and the place since.

## Calliope

`naming.rs` already runs per-culture fragment generators (mewo2-class) and
`settlements.rs` already scores site quality (confluence, delta, harbor,
mineral pull). Missing is exactly the time dimension.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Hydronym conservatism: rivers/mountains named from the *oldest* culture layer in a region; conquest renames settlements but never rivers | S | The single most authentic toponymic rule known |
| 2 | Bounded conquest layers: when a culture expands (M4 wars), its suffix set applies inside the conquered polygon only — Danelaw-style clustering | M | Borders become readable in the names themselves |
| 3 | Age-keyed name erosion: phonological softening passes ∝ settlement age, so old capitals sound worn, colonies transparent | S | Cheap, striking, linguistically honest |
| 4 | Compositional etymology stored with each name (founder/descriptor + suffix) and surfaced as gloss in the inspector | S | The "if you have to ask" layer for names |
| 5 | Path-dependent persistence: settlements keep rank after their founding resource depletes; chronicle notes the obsolete reason | S | Already half-true via mining camps; make it deliberate |
| 6 | Von Thünen ring texture (land-use rings around towns) in satellite render | M | Instant lived-in countryside; pairs with 08 crop packages |
