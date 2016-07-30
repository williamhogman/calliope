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
   gc.temperate-decidous-forest (color 0 137 158)
   gc.temperate-rain-forest (color 105 245 196)
   gc.temperate-desert (color 255 241 46)
   gc.shrubland (color 172 250 178)
   gc.taiga (color 15 199 255)
   gc.snow (color 255 255 255)
   gc.scorched-land (color 168 91 39)
   gc.tundra (color 128 127 121)
   gc.bare-land (color 242 240 94)})

(defn biome->color [biome]
  (print biome)
  (get biome-color-map biome))

(def world->pixels (np.vectorize biome->color))
