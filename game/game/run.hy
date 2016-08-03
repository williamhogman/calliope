(import [game.color :as color]
        [game.geo :as geo]
        [numpy :as np]
        [scipy.misc :as smp]
        [matplotlib.pyplot :as plt]
        [game [engine]])

(defn freqs [x]
  (let [y (.flatten x)] (print y.shape))
  (let [bc (-> x .flatten np.bincount)]
    bc))

(defn run []
  (engine.start))
