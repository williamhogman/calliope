(import [operator [itemgetter]]
        [noise [snoise3]]
        [numpy :as np]
        [numpy [floor dstack apply-over-axes vectorize multiply subtract bincount add]]
        [game.constants :as gc])

(def SIZE 1024)
(def LAYERS 3)

(defn dupe-elements [l rep]
  (flatten (list-comp (take rep (repeat x)) (x l))))

(defn biome-table-row-conv [x]
  (let [l (len x)]
    (cond
     [(= l 6) x]
     [(= l 3) (dupe-elements x 2)]
     [(= l 2) (dupe-elements x 3)]
     [(= l 1) (dupe-elements x 6)])))

(defn to-biome-table [&rest xs]
  (->
   (map biome-table-row-conv xs)
   list
   np.array))

(def biome-table
  (to-biome-table
   [gc.ice]
   [gc.tundra]
   [gc.grassland gc.grassland gc.woodland gc.boreal-forest gc.boreal-forest gc.boreal-forest]
   [gc.desert gc.desert gc.woodland gc.woodland gc.seasonal-rain-forest gc.temperate-rain-forest]
   [gc.desert gc.savanna gc.tropical-rain-forest]
   [gc.desert gc.savanna gc.tropical-rain-forest]))

(defn get-in-biome-table [[a b]]
  (if (< a 0)
    gc.water
    (get biome-table (, a b))))

(def snoise3* (vectorize (fn [x y z]
                           (snoise3
                            (* (/ x SIZE) 5)
                            (* (/ y SIZE) 5)
                            z 4))))
(defn radial []
  (let [xc (np.linspace (- np.pi) (* np.pi 3) SIZE)
        yc (np.linspace 0 (* np.pi 1) SIZE)
        [x y] (np.meshgrid xc yc)]
    (* (np.cos x) (np.sin y))))


;; Temperature
;; -1 = -50C
;; +1 = +50C

(defn temperature [l]
  (let [mid (/ 2 SIZE)
        d-mid (abs (- mid l))]
    (/ (- mid d-mid) mid)))

(defn temperature-mtx [size]
  (let [top (np.linspace 0 1 (/ size 2))
        bottom (np.flipud top)
        combined (np.hstack [top bottom])
        tiled (np.tile combined (, size 1))]
    (np.transpose tiled)))

;; Temperature decline is about 10c per 1000m
;; Because height = 1 <-> 4000m, 1000m = 0.25height units
;; 0.25 height units = -0.2 temperature units
;; 1 height units = -0.8 temperature units
(defn height-adjusted-temperatures [temperatures heights]
  (let [height+ (np.minimum 0 heights)]
    (+ temperatures (* -0.8 height+))))

;; Heights, we consider 1.0 == 4000 metres, likewise -1.0 is -4000 metres

(defn generate-layers []
  (let [mtx (np.fromfunction snoise3* [SIZE SIZE LAYERS])
        layers (np.dsplit mtx LAYERS)
        rad (.reshape (radial) [SIZE SIZE 1])
        height (/ (+ rad (first layers)) 2)
        moisture (second layers)
        moistureM (-> moisture
                      (+ 1)
                      (* 3)
                      floor
                      (.astype int))
        heightM (-> height
                    (* 6)
                    floor
                    (.astype int))
        base-temperature (temperature-mtx SIZE)
        temperature (height-adjusted-temperatures base-temperature height)
        combined (dstack [heightM moistureM])
        biomes (np.apply-along-axis get-in-biome-table 2 combined)]
    {:biomes biomes
     :temperature temperature
     :height height}))
