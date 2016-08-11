(import [numpy :as np]
        [game.constants :as gc])

(defn color [r g b]
  (tuple [r g b]))

(def biome-color-map
  {gc.water (color 0 105 148)
   gc.subtropical-desert (color 242 186 56)
   gc.grassland (color 91 201 157)
   gc.tropical-seasonal-forest (color 0 158 3)
   gc.tropical-rain-forest (color 0 82 1)
   gc.temperate-desert (color 204 156 0)
   gc.temperate-decidous-forest (color 0 137 108)
   gc.temperate-rain-forest (color 105 245 100)
   gc.temperate-desert (color 255 241 46)
   gc.shrubland (color 172 250 150)
   gc.taiga (color 15 199 199)
   gc.snow (color 255 255 255)
   gc.scorched-land (color 168 91 39)
   gc.tundra (color 128 127 121)
   gc.bare-land (color 242 240 94)})

(defn biome->color [biome]
  (get biome-color-map biome))

(def world->pixels (np.vectorize biome->color [np.uint8 np.uint8 np.uint8]))
