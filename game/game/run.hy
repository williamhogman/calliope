(import [game.color :as color]
        [game.geo :as geo]
        [scipy.misc :as smp])

(defn run []
  (let [w (geo.generate-layers)
        pixs (color.world->pixels w)
        img (smp.toimage pixs)]
    (.show img)))
