(import [game.color :as color]
        [game.geo :as geo]
        [numpy :as np]
        [scipy.misc :as smp]
        [matplotlib.pyplot :as plt])

(defn freqs [x]
  (let [y (.flatten x)] (print y.shape))
  (let [bc (-> x .flatten np.bincount)]
    bc))

(defn run []
  (let [w (geo.generate-layers)
        pixs (color.world->pixels w)
        img (smp.toimage pixs)]
    (.show img)))
