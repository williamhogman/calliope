;;
;; Resources
;;
(import itertools)

(defn ensure-col [x]
  (if-not
   (coll? x)
   [x]
   x))

(defn to-triple [into [as r bs]]
  (let [combined (itertools.product (ensure-col as) (ensure-col bs))]
    (reduce
     (fn [cur [a b]]
       (print a r b)
       (assoc cur
              (, r :right a) b
              (, r :left b) a)
       cur)
     combined into)))

(defmacro triples [&rest xs]
  (let [as-strs (map (fn [x] (if (coll? x)
                               (list (map str x))
                               (str x))) xs)
        raw-triples (list (partition (list as-strs) 3))]
    `(reduce to-triple ~raw-triples {})))

(def resources
  (triples
   bananas ISA fruit

   blueberries ISA berry
   strawberries ISA berry
   blackberries ISA berry

   berry ISA fruit

   berry REQUIRES gathering

   fruit ISA food

   cattle ISA livestock
   sheep ISA livestock
   horse ISA livestock
   pig ISA livestock

   deer ISA game
   elk ISA game

   [livestock game] ISA animal

   animal ISA food

   coal ISA fuel
   copper ISA metal
   silver ISA metal
   gold ISA metal
   iron ISA metal
   mithril ISA metal

   coal ABUNDANCE common
   copper ABUNDANCE common
   iron ABUNDANCE common
   silver ADBUNDANCE uncommon
   gold ABUNDANCE rare

   metal REQUIRES metal-working
   iron REQUIRES iron-working

   metal FOUNDIN montain))

(print resources)
