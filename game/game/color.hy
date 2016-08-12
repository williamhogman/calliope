(import [numpy :as np]
        [game.constants :as gc])

(defn color [r g b]
  (tuple [r g b]))

(def biome-color-map
  {gc.water (color 0 38 255)
   gc.desert (color 242 186 56)
   gc.savanna (color 168 91 39)
   gc.tropical-rain-forest (color 0 82 1)
   gc.grassland (color 91 201 157)
   gc.woodland (color 91 201 157)
   gc.seasonal-rain-forest (color 0 82 1)
   gc.temperate-rain-forest (color 0 59 122)
   gc.boreal-forest (color 45 89 3)
   gc.tundra (color 155 155 155)
   gc.ice (color 255 255 255)})

(defn biome->color [biome]
  (get biome-color-map biome))

(def world->pixels (np.vectorize biome->color [np.uint8 np.uint8 np.uint8]))
