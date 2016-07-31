(import [operator [itemgetter]]
        [noise [snoise3]]
        [numpy :as np]
        [numpy [floor dstack apply-over-axes vectorize multiply subtract bincount add]]
        [game.constants :as gc])

(def SIZE 1024)
(def LAYERS 3)

(def biome-table
  (np.array
   [[gc.subtropical-desert gc.grassland gc.tropical-seasonal-forest gc.tropical-rain-forest]
    [gc.temperate-desert gc.grassland gc.temperate-decidous-forest gc.temperate-rain-forest]
    [gc.temperate-desert  gc.shrubland gc.taiga gc.taiga]
    [gc.scorched-land gc.bare-land gc.tundra gc.snow]]))

(defn get-in-biome-table [[a b]]
  (if (< a 0)
    gc.water
    (get biome-table (tuple* a b))))

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

(defn tuple* [&rest a]
  (tuple a))

(defn generate-layers []
  (let [mtx (np.fromfunction snoise3* [SIZE SIZE LAYERS])
        layers (np.dsplit mtx LAYERS)
        rad (.reshape (radial) [SIZE SIZE 1])
        height (/ (+ rad (first layers)) 2)
        moisture (second layers)
        moistureM (-> moisture
                      (add 1)
                      (multiply 2)
                      floor
                      (.astype int))
        heightM (-> height
                   (multiply 4)
                   floor
                   (.astype int))
        combined (dstack [heightM moistureM])]
    (np.apply-along-axis get-in-biome-table 2 combined)))
